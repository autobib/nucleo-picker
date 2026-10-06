//! A find example with asynchronous file previews
//!
//! Run with:
//!
//! ```bash
//! cargo run --release --features preview --example find_preview -- [directory]
//! ```
//!
//! This implementation generates previews for files which are valid UTF-8, and prints read errors
//! directly into the preview buffer.
//!
//! This example uses `PoolPreviewer` to perform the preview worker management. See the
//! `find_preview_full` example for the equivalent manual worker pool, which may be useful for
//! reference for more complex `Preview` implementations.

use std::{
    borrow::Cow,
    convert::Infallible,
    env::args_os,
    fs::File,
    io::{self, Read},
    num::NonZeroUsize,
    path::PathBuf,
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crossterm::style::{ContentStyle, Stylize};
use ignore::{DirEntry, WalkBuilder, WalkState};
use nucleo_picker::{
    PickerOptions, Render,
    preview::{
        PoolPreviewer, Preview, PreviewBuffer, PreviewWorker,
        request::{PreviewRequest, PreviewResponse},
    },
};

// maximum size of a preview
const PREVIEW_BYTES: usize = 256 * 1024;

struct DirEntryRender;

impl Render<DirEntry> for DirEntryRender {
    type Str<'a> = Cow<'a, str>;

    fn render<'a>(&self, entry: &'a DirEntry) -> Self::Str<'a> {
        entry.path().to_string_lossy()
    }
}

// The internal `PoolPreviewer` does most of the work, but we use this wrapper type since we can
// handle certain file types directly without IO and immediately return.
struct FilePreview {
    pool: PoolPreviewer<DirEntry>,
}

impl FilePreview {
    fn new() -> io::Result<Self> {
        Ok(Self {
            pool: PoolPreviewer::new(FileWorker::default(), NonZeroUsize::new(4).unwrap())?,
        })
    }
}

impl Preview<DirEntry> for FilePreview {
    // there are no fatal errors: all errors are reported directly into the PreviewBuffer.
    // returning an abort error here would abort the picker unnecessarily
    type AbortErr = Infallible;

    fn preview(
        &mut self,
        entry: &DirEntry,
        request: PreviewRequest<'_, DirEntry>,
        timeout: Duration,
    ) -> Result<PreviewResponse, Self::AbortErr> {
        // entry metadata is already available in the dir entry so we can label non-files synchronously
        let label = match entry.file_type() {
            Some(kind) if kind.is_file() => None,
            Some(kind) if kind.is_dir() => Some("[Directory]"),
            Some(kind) if kind.is_symlink() => Some("[Symbolic link]"),
            _ => Some("[Not a regular file]"),
        };
        if let Some(label) = label {
            let mut buffer = request.ready();
            // push the information label
            buffer.push_styled_str(label, ContentStyle::new().dim().italic());
            return Ok(PreviewResponse::Ready(buffer));
        }

        // let the preview pool handle everything else
        self.pool.preview(entry, request, timeout)
    }
}

/// A single preview worker. It holds scratch space for reading from the file.
#[derive(Clone, Default)]
struct FileWorker {
    bytes: Vec<u8>,
}

impl PreviewWorker<DirEntry> for FileWorker {
    fn preview(
        &mut self,
        entry: &DirEntry,
        buffer: &mut PreviewBuffer,
        is_cancelled: impl Fn() -> bool,
    ) {
        if let Err(err) = read_preview(entry, &mut self.bytes, buffer, is_cancelled) {
            buffer.clear();
            buffer.set_err(true);
            buffer.push_text(&format!(
                "Cannot preview {}:\n{err}",
                entry.path().display()
            ));
        }
    }
}

fn read_preview(
    entry: &DirEntry,
    bytes: &mut Vec<u8>,
    buffer: &mut PreviewBuffer,
    is_cancelled: impl Fn() -> bool,
) -> io::Result<()> {
    let mut file = File::open(entry.path())?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("Path is no longer a regular file"));
    }
    bytes.clear();
    let mut chunk = [0; 8192];
    // we periodically check `is_cancelled` cooperatively to avoid doing too much extra work if
    // we can avoid it
    while bytes.len() <= PREVIEW_BYTES {
        if is_cancelled() {
            return Ok(());
        }
        // read one extra byte to distinguish a perfectly full preview from a truncated file
        let capacity = chunk.len().min(PREVIEW_BYTES + 1 - bytes.len());
        let count = match file.read(&mut chunk[..capacity]) {
            Ok(0) => break,
            Ok(count) => count,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        bytes.extend_from_slice(&chunk[..count]);
    }
    if is_cancelled() {
        return Ok(());
    }
    let truncated = bytes.len() > PREVIEW_BYTES;
    let prefix = &bytes[..bytes.len().min(PREVIEW_BYTES)];
    // in this example we just use invalid UTF-8 for the 'binary/non-UTF-8' heuristic
    let text = match std::str::from_utf8(prefix) {
        Ok(text) => Some(text),
        // make sure not to split a UTF-8 char with the size truncation!
        Err(err) if truncated && err.error_len().is_none() => {
            std::str::from_utf8(&prefix[..err.valid_up_to()]).ok()
        }
        Err(_) => None,
    };
    if let Some(text) = text.filter(|_| !prefix.contains(&0)) {
        buffer.set_line_numbers(true);
        buffer.push_text(text);
        if truncated {
            buffer.newline();
            buffer.push_styled_str(
                "[Preview truncated at 256 KiB]",
                ContentStyle::new().dim().italic(),
            );
        }
    } else {
        buffer.push_styled_str("[Binary file]", ContentStyle::new().dim().italic());
    }
    Ok(())
}

fn main() -> io::Result<ExitCode> {
    let root = args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);

    let mut picker = PickerOptions::new().match_paths().picker(DirEntryRender);

    let previewer = FilePreview::new()?;
    let injector = picker.injector();
    let mut picker = picker.with_preview(previewer);

    let stop = Arc::new(AtomicBool::new(false));
    let walker_stop = Arc::clone(&stop);
    // remember to move the injector in so it drops when directory traversal is done!
    thread::spawn(move || {
        let stop = &walker_stop;
        WalkBuilder::new(root).build_parallel().run(|| {
            let injector = injector.clone();
            Box::new(move |entry| {
                if stop.load(Ordering::Relaxed) {
                    return WalkState::Quit;
                }
                if let Ok(entry) = entry {
                    injector.push(entry);
                }
                WalkState::Continue
            })
        });
    });

    let result = picker.pick();

    // tell the directory walker to shut down
    stop.store(true, Ordering::Relaxed);

    Ok(match result? {
        Some(entry) => {
            println!("{}", entry.path().display());
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("No path selected!");
            ExitCode::FAILURE
        }
    })
}
