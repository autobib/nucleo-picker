use std::{
    fs, io,
    path::PathBuf,
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use nucleo_picker::{
    Picker,
    preview::{
        ItemRef, Preview, PreviewBuffer, PreviewWorker,
        request::{PreviewRequest, PreviewResponse, QueuedPreviewRequest},
    },
    render::PathRenderer,
};
use parking_lot::{Condvar, Mutex};

// a single slot for a preview request
struct State<T> {
    request: Option<QueuedPreviewRequest<T>>,
    closed: bool,
}

// state shared between the picker and the previewer
struct Shared<T> {
    state: Mutex<State<T>>,
    available: Condvar,
}

// the previewer itself
struct SinglePreviewer<T> {
    shared: Arc<Shared<T>>,
    worker: Option<JoinHandle<()>>,
}

impl<T: Send + Sync + 'static> SinglePreviewer<T> {
    // initialize by spawning a background thread and moving the worker there
    fn from_worker<W>(worker: W) -> io::Result<Self>
    where
        W: PreviewWorker<T> + Send + 'static,
    {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                request: None,
                closed: false,
            }),
            available: Condvar::new(),
        });
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new().spawn(move || work(&worker_shared, worker))?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }
}

impl<T: Send + Sync + 'static> Preview<T> for SinglePreviewer<T> {
    fn preview(&mut self, request: PreviewRequest<'_, T>, _timeout: Duration) -> PreviewResponse {
        // the worker may have panicked
        if self.worker.as_ref().unwrap().is_finished() {
            if let Err(panic) = self.worker.take().unwrap().join() {
                std::panic::resume_unwind(panic);
            }
            unreachable!("preview worker exited before shutdown");
        }

        // defer the request, and sent the QueuedPreviewRequest to the background thread
        let (pending, request) = request.defer();
        let previous = self.shared.state.lock().request.replace(request);

        // notify the worker that there is a new request
        self.shared.available.notify_one();

        // dropping a request is very fast (unless it owns the only handle to the item pool, which
        // will never be the case while the picker is running) but it's still good practice to drop
        // outside the mutex
        drop(previous);
        PreviewResponse::Pending(pending)
    }

    fn focus_changed(&mut self, item: Option<ItemRef<'_, T>>) {
        // use focus notifications to pop an outdated request, so the worker
        // doesn't accidentally pick up an outdated request
        let focused = item.map(|item| item.id());
        let previous = self
            .shared
            .state
            .lock()
            .request
            .take_if(|request| Some(request.id()) != focused);
        drop(previous);
    }
}

impl<T> Drop for SinglePreviewer<T> {
    fn drop(&mut self) {
        let queued = {
            let mut state = self.shared.state.lock();
            state.closed = true;
            state.request.take()
        };
        self.shared.available.notify_one();
        drop(queued);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn work<T: Send + Sync + 'static, W: PreviewWorker<T>>(shared: &Shared<T>, mut worker: W) {
    // the actual worker thread

    // a buffer which we will re-use to avoid allocations
    let mut buffer = PreviewBuffer::new();

    loop {
        // acquire new work
        let queued = {
            let mut state = shared.state.lock();
            loop {
                if state.closed {
                    return;
                }
                if let Some(request) = state.request.take() {
                    break request;
                }
                shared.available.wait(&mut state);
            }
        };

        // claim that we will work on it. maybe the picker has cancelled the request in the
        // meantime, in which case there's nothing to do
        let Ok(request) = queued.start() else {
            continue;
        };

        if !request.is_cancelled() {
            // failed publication leaves the previous preview in the buffer.
            buffer.clear();

            // make cancellation available cooperatively to the worker and generate the preview
            worker.preview(request.item(), &mut buffer, || request.is_cancelled());

            // if the request was cancelled, publication will fail anyway, so we don't need to
            // bother checking it ourself
            let _ = request.publish(&mut buffer);
        }
    }
}

// a simple example generating previews from file contents
struct FileWorker;

impl PreviewWorker<PathBuf> for FileWorker {
    fn preview(
        &mut self,
        path: &PathBuf,
        buffer: &mut PreviewBuffer,
        _is_cancelled: impl Fn() -> bool,
    ) {
        // don't do this for large files
        match fs::read_to_string(path) {
            Ok(text) => buffer.push_text(&text),
            Err(error) => {
                buffer.set_err(true);
                buffer.push_text(&error.to_string());
            }
        }
    }
}

fn main() -> io::Result<()> {
    let picker = Picker::new(PathRenderer);
    picker.push_batch([PathBuf::from("Cargo.toml"), PathBuf::from("README.md")]);
    let previewer = SinglePreviewer::from_worker(FileWorker)?;
    let mut picker = picker.with_preview(previewer);
    if let Some(path) = picker.pick()? {
        println!("{}", path.display());
    }
    Ok(())
}
