//! A find example with asynchronous file previews
//!
//! Run with:
//!
//! ```bash
//! cargo run --release --features preview --example find-preview-full -- [directory]
//! ```
//!
//! This implementation generates previews for files which are valid UTF-8, and prints read errors
//! directly into the preview buffer.
//!
//! This example could be simplified by using `PoolPreviewer`; see `find-preview` for the details. The
//! implementation here is provided to give a somewhat simplified by complete reference for implementing `Preview`.
//! Note that some details are omitted here, mainly around panic handling and job queue growth
//! management.

use std::{
    borrow::Cow,
    convert::Infallible,
    env::args_os,
    fs::File,
    io::{self, Read},
    path::PathBuf,
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crossterm::style::{ContentStyle, Stylize};
use ignore::{DirEntry, WalkBuilder, WalkState};
use nucleo_picker::{
    PickerOptions, Render,
    preview::{
        Preview, PreviewBuffer,
        request::{ActivePreviewRequest, PreviewRequest, PreviewResponse, QueuedPreviewRequest},
    },
};
use parking_lot::{Condvar, Mutex};

// the number of preview workers
const WORKERS: usize = 4;
// maximum size of a preview
const PREVIEW_BYTES: usize = 256 * 1024;

struct DirEntryRender;

impl Render<DirEntry> for DirEntryRender {
    type Str<'a> = Cow<'a, str>;

    fn render<'a>(&self, entry: &'a DirEntry) -> Self::Str<'a> {
        entry.path().to_string_lossy()
    }
}

struct Shared {
    // This is a LIFO queue for preview jobs. If this is `None`, the thread is closed
    jobs: Mutex<Option<Vec<QueuedPreviewRequest<DirEntry>>>>,
    available: Condvar,
}

struct FilePreview {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

impl FilePreview {
    fn new() -> io::Result<Self> {
        let mut pool = Self {
            shared: Arc::new(Shared {
                jobs: Mutex::new(Some(Vec::new())),
                available: Condvar::new(),
            }),
            workers: Vec::with_capacity(WORKERS),
        };
        // construct the pool before spawning so Drop joins existing workers if a later spawn fails.
        for _ in 0..WORKERS {
            let shared = Arc::clone(&pool.shared);
            pool.workers
                .push(thread::Builder::new().spawn(move || work(&shared))?);
        }
        Ok(pool)
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
        _timeout: Duration,
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
        // defer file IO to the worker pool to not delay the frame render
        let (pending, request) = request.defer();
        {
            let mut jobs = self.shared.jobs.lock();
            let jobs = jobs.as_mut().unwrap();
            // for large queues, it may be worthwhile to amortize this step; for example, this is
            // done internally by the `PoolPreviewer`.
            jobs.retain(|request| !request.is_cancelled());
            jobs.push(request);
        }
        self.shared.available.notify_one();
        Ok(PreviewResponse::Pending(pending))
    }
}

impl Drop for FilePreview {
    fn drop(&mut self) {
        // the picker cancels outstanding requests before dropping the previewer, so active workers will observe cancellation
        let queued = self.shared.jobs.lock().take();
        self.shared.available.notify_all();
        drop(queued);
        // join after releasing the lock so workers can finish
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn work(shared: &Shared) {
    let mut bytes = Vec::new();
    let mut buffer = PreviewBuffer::new();
    loop {
        let job = {
            let mut jobs = shared.jobs.lock();
            loop {
                let Some(queue) = jobs.as_mut() else {
                    return;
                };
                // remember this is a LIFO queue: the newest request is the highest priority so we
                // process that first
                if let Some(job) = queue.pop() {
                    break job;
                }
                shared.available.wait(&mut jobs);
            }
        };
        // 'starting' means that a worker has picked up the job: this prevents the picker from
        // cancelling the job for reprioritization since we are already working on it. if this is
        // `None` it means that the job was already cancelled while it was queued
        let Some(request) = job.start() else {
            continue;
        };
        if request.is_cancelled() {
            continue;
        }
        // make sure to clear the buffer in case publication failed last time
        buffer.clear();
        let result = read_preview(&request, &mut bytes, &mut buffer);
        if let Err(err) = result {
            // the buffer might have been partially written by `read_preview`
            buffer.clear();
            buffer.set_err(true);
            // use `push_text` here for untrusted content and error text since it will handle
            // newlines, control chars, etc. without corrupting the picker screen
            buffer.push_text(&format!(
                "Cannot preview {}:\n{err}",
                request.item().path().display()
            ));
        }
        // since publication can technically race with cancellation, we have to swap in our reusable
        // buffer, but we don't actually care if it succeeds or not. if it fails the picker will
        // manage retries automatically for us
        let _ = request.publish(&mut buffer);
    }
}

fn read_preview(
    request: &ActivePreviewRequest<DirEntry>,
    bytes: &mut Vec<u8>,
    buffer: &mut PreviewBuffer,
) -> io::Result<()> {
    let mut file = File::open(request.item().path())?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("Path is no longer a regular file"));
    }
    bytes.clear();
    let mut chunk = [0; 8192];
    // we periodically check `is_cancelled` cooperatively to avoid doing too much extra work if
    // we can avoid it
    while bytes.len() <= PREVIEW_BYTES {
        if request.is_cancelled() {
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
    if request.is_cancelled() {
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
