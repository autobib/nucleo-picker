use std::{
    any::Any,
    convert::Infallible,
    io,
    marker::PhantomData,
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
/// threadpool. Each worker lives in its own thread and manages its own scratch space. There are no
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
    /// If this method panics, the panic will cause the preview threadpool to shut down.
    /// On the next preview request, the panic will be propagated to the picker itself.
    fn preview(&mut self, item: &T, buffer: &mut PreviewBuffer, is_cancelled: impl Fn() -> bool);
}

/// A dedicated thread-pool for handling preview requests without blocking.
///
/// This struct manages a background threadpool of [`PreviewWorker`]s and dispatches preview work
/// without blocking.
///
/// This trait is designed around ease of use: just implement [`PreviewWorker`] and initialize
/// with [`PoolPreviewer::new`]. However, this means that a number of more complex use-cases are not
/// supported:
///
/// - Communication between workers.
/// - More complex request management (such as deferring or dropping requests after beginning work).
/// - Coalescing related requests.
/// - Fallible preview generation.
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
/// ## Parametrized error
///
/// There is a parametrized abort error type `A` (which defaults to [`Infallible`]). This is for
/// compatibility with arbitrary application abort errors. Note that the
/// [`PoolPreviewer::new`] constructor uses the [`Infallible`] default; use
/// [`PoolPreviewer::with_abort`] to change the error type.
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
/// let previewer = PoolPreviewer::new(FilePreview, NonZeroUsize::new(2).unwrap())?;
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
/// If a worker panics, this thread-pool is shut down and the panic is stored internally to the
/// preview pool. The next call to `preview` will immediately propagate the panic. However, note
/// that the picker can return between the panic and the next call to `preview`, so the panic
/// might not be observed by the picker.
pub struct PoolPreviewer<T, A = Infallible> {
    pool: Pool<T>,
    abort: PhantomData<fn() -> A>,
}

// Keep shutdown with the pool so changing the abort type can move it without stopping workers.
struct Pool<T> {
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

impl<T, A> PoolPreviewer<T, A> {
    /// Set an application-defined error type.
    ///
    /// This method is intended for compatibility with an application-defined error.
    /// Preview generation is infallible, regardless of the error type.
    #[must_use]
    pub fn with_abort<B>(self) -> PoolPreviewer<T, B> {
        PoolPreviewer {
            pool: self.pool,
            abort: PhantomData,
        }
    }

    /// Report whether this pool is closed.
    ///
    /// If this method returns `true`, it means the pool is closed and can no longer handle preview
    /// requests. The pool will only close if a preview worker panics while generating a preview.
    ///
    /// On the other hand, a worker may still panic after observing `false` but before using this pool.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.pool.shared.state.lock().jobs.is_none()
    }
}

impl<T: Send + Sync + 'static> PoolPreviewer<T> {
    /// Create a preview thread-pool with the requested number of threads and the provided worker.
    ///
    /// At startup, the worker is cloned `threads - 1` times and sent to each worker thread.
    pub fn new<W>(worker: W, threads: NonZeroUsize) -> io::Result<Self>
    where
        W: PreviewWorker<T> + Clone + Send + 'static,
    {
        let mut pool = Pool {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    jobs: Some(Vec::new()),
                    panic: None,
                }),
                available: Condvar::new(),
            }),
            workers: Vec::with_capacity(threads.get()),
            prune_at: MIN_PRUNE_AT,
        };
        for _ in 1..threads.get() {
            pool.spawn(worker.clone())?;
        }
        pool.spawn(worker)?;
        Ok(Self {
            pool,
            abort: PhantomData,
        })
    }
}

impl<T: Send + Sync + 'static> Pool<T> {
    fn spawn<W>(&mut self, worker: W) -> io::Result<()>
    where
        W: PreviewWorker<T> + Send + 'static,
    {
        let shared = Arc::clone(&self.shared);
        self.workers.push(thread::Builder::new().spawn(move || {
            // Catch around the whole worker lifetime: a panicking worker is never reused.
            if let Err(panic) = catch_unwind(AssertUnwindSafe(|| work(&shared, worker))) {
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

impl<T: Send + Sync + 'static, A> Preview<T> for PoolPreviewer<T, A> {
    type AbortErr = A;

    fn preview(
        &mut self,
        request: PreviewRequest<'_, T>,
        timeout: Duration,
    ) -> Result<PreviewResponse, Self::AbortErr> {
        let _ = timeout;
        let (pending, request) = request.defer();
        let pool = &mut self.pool;
        {
            let mut state = pool.shared.state.lock();
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
            if jobs.len() >= pool.prune_at {
                jobs.retain(|request| !request.is_cancelled());
                // reset `prune_at` to allow a number of new items proportional to the current queue
                // size so we don't re-scan the backlog on every push. note that the rest is done
                // *after dropping cancelled jobs*
                pool.prune_at = MIN_PRUNE_AT.max(jobs.len().saturating_mul(2));
            }
            jobs.push(request);
        }
        pool.shared.available.notify_one();
        Ok(PreviewResponse::Pending(pending))
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
