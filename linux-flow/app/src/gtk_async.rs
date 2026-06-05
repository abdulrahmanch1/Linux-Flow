//! Runs blocking or async work off the GTK main loop and delivers the result
//! back on it, without freezing the UI. Several wizard and settings flows share
//! this exact thread + channel + poll shape; centralizing it keeps the plumbing
//! in one place so a change is made once instead of being copied per call site.

use std::future::Future;
use std::sync::mpsc;
use std::time::Duration;

use gtk4::glib;

const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Runs `work` on a dedicated background thread, then calls `on_result` with its
/// output back on the GTK thread. The GTK loop is polled every 50 ms so it never
/// blocks while `work` is in flight; `on_result` runs at most once.
///
/// If the worker thread vanishes before delivering (a panic — practically
/// impossible here, since `work` always returns a value), the poller just stops.
pub(crate) fn run_worker<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    on_result: impl FnOnce(T) + 'static,
) {
    let (tx, rx) = mpsc::channel::<T>();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });

    let mut on_result = Some(on_result);
    glib::timeout_add_local(POLL_INTERVAL, move || match rx.try_recv() {
        Ok(out) => {
            if let Some(f) = on_result.take() {
                f(out);
            }
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
    });
}

/// Builds a private current-thread Tokio runtime and blocks on `fut`. A
/// runtime-build failure is mapped through `on_build_err` into the same output
/// type, so callers handle a single shape. Call this from inside a
/// [`run_worker`] closure to drive async work without a shared runtime.
pub(crate) fn block_on_new_rt<T>(
    fut: impl Future<Output = T>,
    on_build_err: impl FnOnce(String) -> T,
) -> T {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt.block_on(fut),
        Err(e) => on_build_err(e.to_string()),
    }
}
