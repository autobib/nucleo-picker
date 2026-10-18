//! A Rust RFC brower
//!
//! Dynamically load live Rust RFCs into the picker for browsing.
//!
//! Run with:
//!
//! ```bash
//! cargo run --release --features preview --example online_preview
//! ```
//!
//! This example exists in particular to highlight idiomatic handling of fatal preview errors,
//! for instance if the network disconnects.

use std::{
    fmt::{self, Write as _},
    io::{self, BufWriter, IsTerminal, Read},
    num::NonZeroUsize,
    process::ExitCode,
    str::Utf8Error,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Sender, channel},
    },
    thread,
    time::Duration,
};

use nucleo_picker::{
    Injector, Picker, Render,
    error::PickError,
    event::{Event, StdinEventSender},
    preview::{PoolPreviewer, PreviewBuffer, PreviewWorker},
};
use serde::Deserialize;

const WORKERS: usize = 6;
const CATALOG_URL: &str = "https://data.jsdelivr.com/v1/package/gh/rust-lang/rfcs@main";
const BASE_URL: &str = "https://cdn.jsdelivr.net/gh/rust-lang/rfcs@main/";

struct Page {
    title: String,
    path: String,
}

struct PageRenderer;

impl Render<Page> for PageRenderer {
    type Str<'a> = &'a str;

    fn render<'a>(&self, page: &'a Page) -> Self::Str<'a> {
        &page.title
    }
}

enum AppError {
    Keyboard(io::Error),
    Catalog(FetchError),
    Preview { path: String, error: FetchError },
}

enum FetchError {
    Http {
        status: u16,
        retry_after: Option<String>,
    },
    Request(ureq::Error),
    InvalidText(Utf8Error),
    InvalidCatalog(&'static str),
}

impl FetchError {
    fn is_fatal(&self) -> bool {
        match self {
            Self::Http { status, .. } => {
                matches!(status, 401 | 403 | 407 | 408 | 429 | 500..=599)
            }
            Self::Request(ureq::Error::TooManyRedirects) | Self::InvalidText(_) => false,
            Self::Request(_) | Self::InvalidCatalog(_) => true,
        }
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http {
                status,
                retry_after,
            } => {
                write!(f, "HTTP {status}")?;
                if let Some(delay) = retry_after {
                    write!(f, " (Retry-After: {delay})")?;
                }
                Ok(())
            }
            Self::Request(error) => error.fmt(f),
            Self::InvalidText(error) => write!(f, "Page is not UTF-8: {error}"),
            Self::InvalidCatalog(message) => write!(f, "Invalid RFC listing: {message}"),
        }
    }
}

fn get(agent: &ureq::Agent, url: &str) -> Result<ureq::http::Response<ureq::Body>, FetchError> {
    let response = agent.get(url).call().map_err(FetchError::Request)?;
    if !response.status().is_success() {
        return Err(FetchError::Http {
            status: response.status().as_u16(),
            retry_after: response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        });
    }
    Ok(response)
}

#[derive(Deserialize)]
struct Catalog {
    files: Vec<CatalogEntry>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum CatalogEntry {
    File { name: String },
    Directory { name: String, files: Vec<Self> },
}

fn load_catalog(
    agent: &ureq::Agent,
    injector: &Injector<Page, PageRenderer>,
    stopped: &AtomicBool,
) -> Result<(), FetchError> {
    if stopped.load(Ordering::Relaxed) {
        return Ok(());
    }
    let catalog: Catalog = get(agent, CATALOG_URL)?
        .body_mut()
        .with_config()
        .read_json()
        .map_err(FetchError::Request)?;
    if stopped.load(Ordering::Relaxed) {
        return Ok(());
    }
    let files = catalog
        .files
        .into_iter()
        .find_map(|entry| match entry {
            CatalogEntry::Directory { name, files } if name == "text" => Some(files),
            _ => None,
        })
        .ok_or(FetchError::InvalidCatalog("missing text directory"))?;

    let mut found = false;
    for entry in files {
        if stopped.load(Ordering::Relaxed) {
            return Ok(());
        }
        let CatalogEntry::File { name } = entry else {
            continue;
        };
        let Some((number, slug)) = name
            .strip_suffix(".md")
            .and_then(|stem| stem.split_once('-'))
        else {
            continue;
        };
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) || slug.is_empty()
        {
            continue;
        }
        let mut title = String::with_capacity(6 + number.len() + slug.len());
        let _ = write!(title, "RFC {number}: ");
        title.extend(slug.chars().map(|ch| match ch {
            '-' | '_' => ' ',
            ch => ch,
        }));
        injector.push(Page {
            title,
            path: format!("text/{name}"),
        });
        found = true;
    }
    if !found {
        return Err(FetchError::InvalidCatalog("no RFCs in text directory"));
    }
    Ok(())
}

#[derive(Clone)]
struct HttpWorker {
    agent: ureq::Agent,
    events: Sender<Event<AppError>>,
    stopped: Arc<AtomicBool>,
    bytes: Vec<u8>,
}

impl PreviewWorker<Page> for HttpWorker {
    fn preview(
        &mut self,
        page: &Page,
        buffer: &mut PreviewBuffer,
        is_cancelled: impl Fn() -> bool,
    ) {
        buffer.set_line_numbers(true);
        if let Err(error) = self.read_preview(page, buffer, is_cancelled) {
            if error.is_fatal() {
                // a fatal error cancels the whole session, but only the first failure actually
                // sends the abort
                if !self.stopped.swap(true, Ordering::Relaxed) {
                    let _ = self.events.send(Event::Abort(AppError::Preview {
                        path: page.path.clone(),
                        error,
                    }));
                }
            } else {
                buffer.set_err(true);
                let _ = write!(
                    buffer.text_writer(),
                    "Cannot preview {BASE_URL}{}:\n{error}",
                    page.path
                );
            }
        }
    }
}

impl HttpWorker {
    fn read_preview(
        &mut self,
        page: &Page,
        buffer: &mut PreviewBuffer,
        is_cancelled: impl Fn() -> bool,
    ) -> Result<(), FetchError> {
        let cancelled = || is_cancelled() || self.stopped.load(Ordering::Relaxed);
        if cancelled() {
            return Ok(());
        }
        let mut response = get(&self.agent, &format!("{BASE_URL}{}", page.path))?;

        self.bytes.clear();
        let mut reader = response.body_mut().as_reader();
        let mut chunk = [0; 8192];
        loop {
            if cancelled() {
                return Ok(());
            }
            let count = match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                // map errors to the shared application error type
                Err(error) => return Err(FetchError::Request(error.into())),
            };
            self.bytes.extend_from_slice(&chunk[..count]);
        }
        if cancelled() {
            return Ok(());
        }
        let text = std::str::from_utf8(&self.bytes).map_err(FetchError::InvalidText)?;
        buffer.push_text(text);
        Ok(())
    }
}

fn main() -> io::Result<ExitCode> {
    let stderr = io::stderr().lock();
    if !stderr.is_terminal() {
        return Err(<PickError>::NotInteractive.into());
    }
    let picker = Picker::new(PageRenderer);

    let (sender, receiver) = channel();
    let stopped = Arc::new(AtomicBool::new(false));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(3)))
        .timeout_global(Some(Duration::from_secs(10)))
        .max_idle_connections_per_host(WORKERS)
        .user_agent(concat!(
            "nucleo-picker-online-preview/",
            env!("CARGO_PKG_VERSION"),
            " (+",
            env!("CARGO_PKG_REPOSITORY"),
            ")"
        ))
        .build()
        .into();
    // the pool, when cloned, is shared by all workers
    let previewer = PoolPreviewer::from_worker(
        HttpWorker {
            agent: agent.clone(),
            events: sender.clone(),
            stopped: Arc::clone(&stopped),
            bytes: Vec::new(),
        },
        NonZeroUsize::new(WORKERS).unwrap(),
    )?;
    let mut picker = picker.with_preview(previewer);

    let stdin_watcher = StdinEventSender::with_default_keybindings(sender.clone());
    thread::Builder::new().spawn(move || {
        if let Err(error) = stdin_watcher.watch() {
            let _ = stdin_watcher
                .into_sender()
                .send(Event::Abort(AppError::Keyboard(error)));
        }
    })?;

    let injector = picker.injector();
    let loader_stopped = Arc::clone(&stopped);
    let loader = thread::Builder::new().spawn(move || {
        if let Err(error) = load_catalog(&agent, &injector, &loader_stopped)
            && !loader_stopped.swap(true, Ordering::Relaxed)
        {
            let _ = sender.send(Event::Abort(AppError::Catalog(error)));
        }
    })?;

    let mut writer = BufWriter::new(stderr);
    let result = picker.pick_with_io(receiver, &mut writer);
    stopped.store(true, Ordering::Relaxed);
    drop(writer);

    // Use `shutdown` instead once ureq supports it:
    // https://github.com/algesten/ureq/issues/641
    let result = report_result(result);
    let (_, previewer) = picker.into_parts();
    thread::spawn(move || {
        drop(previewer);
        let _ = loader.join();
    });
    result
}

fn report_result(result: Result<Option<&Page>, PickError<AppError>>) -> io::Result<ExitCode> {
    match result {
        Ok(Some(page)) => {
            println!("{BASE_URL}{}", page.path);
            Ok(ExitCode::SUCCESS)
        }
        Ok(None) | Err(PickError::UserInterrupted) => Ok(ExitCode::FAILURE),
        Err(error) => {
            match error.factor()? {
                AppError::Keyboard(error) => eprintln!("Keyboard input failed: {error}"),
                AppError::Catalog(error) => {
                    eprintln!("Failed to load RFC catalog from {CATALOG_URL}: {error}");
                }
                AppError::Preview { path, error } => {
                    eprintln!("Online browsing aborted while loading {BASE_URL}{path}: {error}");
                }
            }
            Ok(ExitCode::FAILURE)
        }
    }
}
