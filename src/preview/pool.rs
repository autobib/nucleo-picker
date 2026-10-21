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
    ItemRef, Preview, PreviewBuffer,
    request::{PreviewRequest, PreviewResponse, QueuedPreviewRequest},
};

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
/// Also see [`PoolPreviewer::with_sync_preview`] to add a cheap non-blocking preview callback which
/// can handle certain requests before forwarding them to a preview worker.
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
/// ## Preview wrappers
///
/// Some previewers may benefit from wrapping a [`PoolPreviewer`]. In this case, it is essential
/// that the wrapper type focuses [`Preview::focus_changed`], since the [`PoolPreviewer`] uses this
/// for scheduling to improve responsiveness.
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
pub struct PoolPreviewer<T, F = fn(&T, &mut PreviewBuffer) -> bool> {
    pool: Pool<T>,
    sync_preview: Option<F>,
    prefetch_limit: usize,
}

struct Pool<T> {
    shared: Arc<Shared<T>>,
    workers: Vec<JoinHandle<()>>,
}

struct Shared<T> {
    state: Mutex<State<T>>,
    available: Condvar,
}

struct State<T> {
    jobs: Option<Jobs<T>>,
    idle_workers: usize,
    panic: Option<Box<dyn Any + Send + 'static>>,
}

struct Jobs<T> {
    urgent: Option<QueuedPreviewRequest<T>>,
    background: Vec<QueuedPreviewRequest<T>>,
}

impl<T: Send + Sync + 'static> Jobs<T> {
    fn push_background(&mut self, request: QueuedPreviewRequest<T>, capacity: usize) {
        if capacity == 0 || request.is_cancelled() {
            return;
        }
        let jobs = &mut self.background;
        if jobs.len() == capacity {
            jobs.retain(|request| !request.is_cancelled());
            if jobs.len() == capacity {
                jobs.remove(0);
            }
        }
        jobs.push(request);
    }
}

impl<T, F> PoolPreviewer<T, F> {
    /// Add a synchronous previewer which can handle some preview requests in the picker thread
    /// directly.
    ///
    /// This method is called synchronously, and therefore *must not block*. If it blocks, the
    /// picker will lag. This method is mainly useful when, for some items `T`, slow background work
    /// is not required. This can reduce overhead on the thread-pool.
    ///
    /// If previewing is fast for all items `T`, use a [`SyncPreviewer`](super::SyncPreviewer) instead.
    #[must_use]
    pub fn with_sync_preview<G>(self, preview: G) -> PoolPreviewer<T, G>
    where
        G: FnMut(&T, &mut PreviewBuffer) -> bool,
    {
        PoolPreviewer {
            pool: self.pool,
            sync_preview: Some(preview),
            prefetch_limit: self.prefetch_limit,
        }
    }

    /// Set the prefetch limit.
    ///
    /// The prefetch limit controls how many previews are pre-emptively requested. For slow
    /// previews, setting this to a non-zero value can greatly improve perceived responsiveness
    /// of the interactive previewer.
    ///
    /// The default value is `4`. Setting this to `0` disables prefetch entirely.
    #[must_use]
    pub fn with_prefetch_limit(mut self, limit: usize) -> Self {
        self.prefetch_limit = limit;
        self
    }

    /// Report whether this pool is closed.
    ///
    /// If this method returns `true`, it means the pool is closed and can no longer handle preview
    /// requests. The pool will only close if a preview worker panics during initialization or while generating a preview.
    ///
    /// On the other hand, a worker may still panic after observing `false` but before using this pool.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.pool.is_closed()
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
        pool.pool.spawn(move || first)?;
        for worker in workers {
            pool.pool.spawn(move || worker)?;
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
            pool.pool.spawn(W::default)?;
        }
        Ok(pool)
    }

    fn new(capacity: usize) -> Self {
        Self {
            pool: Pool {
                shared: Arc::new(Shared {
                    state: Mutex::new(State {
                        jobs: Some(Jobs {
                            urgent: None,
                            background: Vec::new(),
                        }),
                        idle_workers: 0,
                        panic: None,
                    }),
                    available: Condvar::new(),
                }),
                workers: Vec::with_capacity(capacity),
            },
            sync_preview: None,
            prefetch_limit: 4,
        }
    }
}

impl<T> Pool<T> {
    fn is_closed(&self) -> bool {
        self.shared.state.lock().jobs.is_none()
    }

    fn resume_panic(&self) -> ! {
        let panic = self.shared.state.lock().panic.take();
        if let Some(panic) = panic {
            resume_unwind(panic);
        }
        panic!("preview worker pool is closed");
    }
}

impl<T: Send + Sync + 'static> Pool<T> {
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

    fn submit(&mut self, request: PreviewRequest<'_, T>, urgent: bool) -> PreviewResponse {
        let (pending, request) = request.defer();
        let background_capacity = 2 * (self.workers.len() - 1);
        {
            let mut state = self.shared.state.lock();
            let Some(jobs) = state.jobs.as_mut() else {
                drop(state);
                self.resume_panic();
            };

            if urgent {
                if let Some(previous) = jobs.urgent.replace(request) {
                    jobs.push_background(previous, background_capacity);
                }
            } else {
                jobs.push_background(request, background_capacity);
            }
        }
        self.shared.available.notify_one();
        PreviewResponse::Pending(pending)
    }
}

impl<T: Send + Sync + 'static, F> PoolPreviewer<T, F>
where
    F: FnMut(&T, &mut PreviewBuffer) -> bool,
{
    fn try_sync(&mut self, request: &mut PreviewRequest<'_, T>) -> bool {
        if let Some(preview) = &mut self.sync_preview {
            if self.pool.is_closed() {
                self.pool.resume_panic();
            }
            if preview(request.item(), &mut request.buffer) {
                return true;
            }
            request.buffer.clear();
        }
        false
    }
}

impl<T: Send + Sync + 'static, F> Preview<T> for PoolPreviewer<T, F>
where
    F: FnMut(&T, &mut PreviewBuffer) -> bool,
{
    fn prefetch_limit(&self) -> usize {
        if self.pool.workers.len() > 1 {
            self.prefetch_limit
        } else {
            0
        }
    }

    fn prefetch(
        &mut self,
        mut request: PreviewRequest<'_, T>,
        _timeout: Duration,
    ) -> Result<PreviewResponse, PreviewBuffer> {
        if self.prefetch_limit() == 0 {
            return Err(request.ready());
        }
        if self.try_sync(&mut request) {
            return Ok(PreviewResponse::Ready(request.ready()));
        }
        Ok(self.pool.submit(request, false))
    }

    fn preview(
        &mut self,
        mut request: PreviewRequest<'_, T>,
        _timeout: Duration,
    ) -> PreviewResponse {
        if self.try_sync(&mut request) {
            return PreviewResponse::Ready(request.ready());
        }
        self.pool.submit(request, true)
    }

    fn focus_changed(&mut self, item: Option<ItemRef<'_, T>>) {
        let focused = item.map(|item| item.id());
        let background_capacity = 2 * (self.pool.workers.len() - 1);
        {
            let mut state = self.pool.shared.state.lock();
            if let Some(jobs) = &mut state.jobs
                && let Some(request) = jobs.urgent.take_if(|request| Some(request.id()) != focused)
            {
                jobs.push_background(request, background_capacity);
            } else {
                return;
            }
        }
        self.pool.shared.available.notify_one();
    }
}

impl<T> Drop for Pool<T> {
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
    shared.state.lock().idle_workers += 1;
    loop {
        let request = {
            let mut state = shared.state.lock();
            loop {
                let can_background = state.idle_workers > 1;
                let Some(jobs) = state.jobs.as_mut() else {
                    return;
                };
                let job = jobs.urgent.take().or_else(|| {
                    if can_background {
                        jobs.background.pop()
                    } else {
                        None
                    }
                });
                if let Some(job) = job {
                    if let Ok(request) = job.start() {
                        state.idle_workers -= 1;
                        break request;
                    }
                } else {
                    shared.available.wait(&mut state);
                }
            }
        };
        if !request.is_cancelled() {
            buffer.clear();
            worker.preview(request.item(), &mut buffer, || request.is_cancelled());
            let _ = request.publish(&mut buffer);
        }
        shared.state.lock().idle_workers += 1;
        shared.available.notify_one();
    }
}

#[cfg(test)]
mod tests;
