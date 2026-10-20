use std::{
    any::Any,
    io,
    num::NonZeroUsize,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use parking_lot::{Condvar, Mutex};

use super::{
    Preview, PreviewBuffer,
    request::{PreviewRequest, PreviewResponse, QueuedPreviewRequest},
};

const MIN_PRUNE_AT: usize = 32;

/// A persistent background preview worker.
///
/// This trait represents the work done to compute a preview cooperatively in a background
/// thread pool. Each worker lives in its own thread and manages its own scratch space. There are no
/// synchronization methods between workers. This trait is designed for use with a [`PoolPreviewer`];
/// read those docs for more detail.
pub trait PreviewWorker<T> {
    /// Generate a preview in the provided buffer.
    ///
    /// This method may safely block to generate the preview. When this method returns, it is assumed
    /// that the buffer contents are ready for publication to the preview pane. If the request is
    /// cancelled while the preview is running, the buffer will not be published.
    ///
    /// The preview should be generated for the provided `item`. The worker may cooperatively check for
    /// early cancellation using the `is_cancelled` callback. This can be done to avoid unnecessary
    /// work on dropped preview requests.
    ///
    /// # Cancellation
    ///
    /// Since cancellation is cooperative, it cannot interrupt an operation which is blocked in the
    /// background. For example, if this preview function makes a network request and the network
    /// hangs without checking for cancellation, then the worker will hang indefinitely.
    ///
    /// If all workers block or hang indefinitely, it will not be possible to generate future previews.
    /// Also, note that workers which hang will cause the [`Drop`] implementation of
    /// [`PoolPreviewer`] to hang while it waits for the workers to finish.
    ///
    /// # Panics
    ///
    /// If this method panics, the panic will cause the preview thread pool to shut down.
    /// On the next preview request, the panic will be propagated to the picker itself.
    fn preview(&mut self, item: &T, buffer: &mut PreviewBuffer, is_cancelled: impl Fn() -> bool);
}

/// A dedicated thread pool for handling preview requests without blocking.
///
/// This struct manages a background thread pool of [`PreviewWorker`]s and dispatches preview work
/// without blocking.
///
/// This previewer is designed around ease of use: just implement [`PreviewWorker`] and initialize using
/// one of three standard methods:
///
/// - From a single clonable worker using [`PoolPreviewer::from_worker`]
/// - From a non-empty iterator of workers using [`PoolPreviewer::from_workers`]
/// - From a default implementation using [`PoolPreviewer::with_default_workers`]
///
/// However, this means that a number of more complex use-cases are not supported:
///
/// - Communication between workers.
/// - More complex request management (such as deferring or dropping requests after beginning work).
/// - Coalescing related requests.
///
/// In such cases, it is advised to implement [`Preview`] directly.
///
/// ## Shutdown
///
/// Dropping the pool discards queued requests, wakes idle workers, and blocks until all worker
/// threads have joined. While picker exit cancels all outstanding requests, it does not terminate
/// running workers since cancellation is cooperative. In particular, if a worker is blocked,
/// dropping the [`PoolPreviewer`] will hang.
///
/// ## Example
///
/// Here is a basic example implementing a file previewer.
///
/// ```no_run
/// use std::{fs, num::NonZeroUsize, path::PathBuf};
/// use nucleo_picker::{
///     Picker,
///     preview::{PoolPreviewer, PreviewBuffer, PreviewWorker},
///     render::PathRenderer,
/// };
///
/// #[derive(Clone)]
/// struct FilePreview;
///
/// impl PreviewWorker<PathBuf> for FilePreview {
///     fn preview(
///         &mut self,
///         path: &PathBuf,
///         buffer: &mut PreviewBuffer,
///         _is_cancelled: impl Fn() -> bool,
///     ) {
///         match fs::read_to_string(path) {
///             Ok(text) => buffer.push_text(&text),
///             Err(error) => {
///                 buffer.set_err(true);
///                 buffer.push_text(&error.to_string());
///             }
///         }
///     }
/// }
///
/// # fn main() -> std::io::Result<()> {
/// let picker = Picker::new(PathRenderer);
/// picker.push_batch([PathBuf::from("Cargo.toml"), PathBuf::from("README.md")]);
/// let previewer = PoolPreviewer::from_worker(FilePreview, NonZeroUsize::new(2).unwrap())?;
/// let mut picker = picker.with_preview(previewer);
/// if let Some(path) = picker.pick()? {
///     println!("{}", path.display());
/// }
/// # Ok(())
/// # }
/// ```
///
/// ## Worker panic
///
/// If a worker panics, this thread pool is shut down and the panic is stored internally to the
/// preview pool. The next call to `preview` will immediately propagate the panic. However, note
/// that the picker can return between the panic and the next call to `preview`, so the panic
/// might not be observed by the picker.
pub struct PoolPreviewer<T> {
    shared: Arc<Shared<T>>,
    workers: Vec<JoinHandle<()>>,
    // the threshold at which the job queue is scanned for cancellation; this is done with
    // amortization
    prune_at: usize,
}

struct Shared<T> {
    state: Mutex<State<T>>,
    available: Condvar,
}

struct State<T> {
    jobs: Option<Vec<QueuedPreviewRequest<T>>>,
    panic: Option<Box<dyn Any + Send + 'static>>,
}

impl<T> PoolPreviewer<T> {
    /// Report whether this pool is closed.
    ///
    /// If this method returns `true`, it means the pool is closed and can no longer handle preview
    /// requests. The pool will only close if a preview worker panics during initialization or while generating a preview.
    ///
    /// On the other hand, a worker may still panic after observing `false` but before using this pool.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.shared.state.lock().jobs.is_none()
    }
}

impl<T: Send + Sync + 'static> PoolPreviewer<T> {
    /// Create a preview thread pool with the requested number of threads and the provided worker.
    ///
    /// At startup, the worker is cloned `threads - 1` times and sent to each worker thread.
    pub fn from_worker<W>(worker: W, threads: NonZeroUsize) -> io::Result<Self>
    where
        W: PreviewWorker<T> + Clone + Send + 'static,
    {
        Self::from_workers(std::iter::repeat_n(worker, threads.get()))
    }

    /// Create a preview thread pool from an iterator of workers.
    ///
    /// One thread is initialized per worker. Note that the iterator must be non-empty. Empty input
    /// will result in an IO error with kind [`io::ErrorKind::InvalidInput`].
    pub fn from_workers<I, W>(workers: I) -> io::Result<Self>
    where
        I: IntoIterator<Item = W>,
        W: PreviewWorker<T> + Send + 'static,
    {
        let mut workers = workers.into_iter();
        let first = workers.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "preview pool requires a worker",
            )
        })?;
        let mut pool = Self::new(workers.size_hint().0.saturating_add(1));
        pool.spawn(move || first)?;
        for worker in workers {
            pool.spawn(move || worker)?;
        }
        Ok(pool)
    }

    /// Create a preview thread pool with workers initialized using [`Default`].
    ///
    /// Each spawned thread calls `W::default()` once. Since the method is called inside the thread,
    /// this means workers do not need to be [`Send`], unlike the other initialization methods.
    ///
    /// This constructor returns once all of the threads have been spawned, but beware that
    /// initialization may still be running in the background. If initialization panics, the panic
    /// is propagated in the usual way and will propagate to the picker on the next preview request.
    pub fn with_default_workers<W>(threads: NonZeroUsize) -> io::Result<Self>
    where
        W: PreviewWorker<T> + Default + 'static,
    {
        let mut pool = Self::new(threads.get());
        for _ in 0..threads.get() {
            pool.spawn(W::default)?;
        }
        Ok(pool)
    }

    fn new(capacity: usize) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    jobs: Some(Vec::new()),
                    panic: None,
                }),
                available: Condvar::new(),
            }),
            workers: Vec::with_capacity(capacity),
            prune_at: MIN_PRUNE_AT,
        }
    }

    fn spawn<F, W>(&mut self, initialize: F) -> io::Result<()>
    where
        F: FnOnce() -> W + Send + 'static,
        W: PreviewWorker<T> + 'static,
    {
        let shared = Arc::clone(&self.shared);
        self.workers.push(thread::Builder::new().spawn(move || {
            // Catch around the whole worker lifetime: a panicking worker is never reused.
            if let Err(panic) = catch_unwind(AssertUnwindSafe(|| {
                let worker = initialize();
                work(&shared, worker);
            })) {
                let discarded = {
                    let mut state = shared.state.lock();
                    (state.jobs.take(), state.panic.replace(panic))
                };
                shared.available.notify_all();
                drop(discarded);
            }
        })?);
        Ok(())
    }
}

impl<T: Send + Sync + 'static> Preview<T> for PoolPreviewer<T> {
    fn preview(&mut self, request: PreviewRequest<'_, T>, _timeout: Duration) -> PreviewResponse {
        let (pending, request) = request.defer();
        {
            let mut state = self.shared.state.lock();
            let Some(jobs) = state.jobs.as_mut() else {
                let panic = state.panic.take();
                drop(state);
                if let Some(panic) = panic {
                    resume_unwind(panic);
                }
                panic!("preview worker pool is closed");
            };

            // if there are too many active jobs, look for opportunities for cancellation
            // in particular, this means the job queue will never be much larger than the picker
            // cache size: once the cache is saturated with N elements, the picker will start
            // cancelling stale requests, which will then be dropped here. the job queue will then
            // be pruned again when it hits size `2N` (or perhaps a bit earlier).
            if jobs.len() >= self.prune_at {
                jobs.retain(|request| !request.is_cancelled());
                // reset `prune_at` to allow a number of new items proportional to the current queue
                // size so we don't re-scan the backlog on every push. note that the rest is done
                // *after dropping cancelled jobs*
                self.prune_at = MIN_PRUNE_AT.max(jobs.len().saturating_mul(2));
            }
            jobs.push(request);
        }
        self.shared.available.notify_one();
        PreviewResponse::Pending(pending)
    }
}

impl<T> Drop for PoolPreviewer<T> {
    fn drop(&mut self) {
        let queued = self.shared.state.lock().jobs.take();
        self.shared.available.notify_all();
        drop(queued);
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn work<T: Send + Sync + 'static, W: PreviewWorker<T>>(shared: &Shared<T>, mut worker: W) {
    let mut buffer = PreviewBuffer::new();
    loop {
        let job = {
            let mut state = shared.state.lock();
            loop {
                let Some(jobs) = state.jobs.as_mut() else {
                    return;
                };
                if let Some(job) = jobs.pop() {
                    break job;
                }
                shared.available.wait(&mut state);
            }
        };
        let Ok(request) = job.start() else {
            continue;
        };
        if request.is_cancelled() {
            continue;
        }
        buffer.clear();
        worker.preview(request.item(), &mut buffer, || request.is_cancelled());
        let _ = request.publish(&mut buffer);
    }
}

#[cfg(test)]
mod tests;
