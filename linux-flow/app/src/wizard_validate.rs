use gtk4::glib;
use zeroize::Zeroizing;

use crate::key_check::{self, KeyCheckError, Provider};

/// Runs a key check off the GTK thread and delivers the verdict back on it.
///
/// Mirrors the spawn-thread + current-thread-runtime + `glib::timeout_add_local`
/// poll pattern used by `wizard_complete`/`wizard_polkit`, so the UI never
/// blocks while the network round-trip is in flight. The key is moved into the
/// worker thread and zeroized there (on drop and inside `key_check::check`).
pub(crate) fn spawn_check(
    provider: Provider,
    key: Zeroizing<String>,
    on_result: impl Fn(Result<(), KeyCheckError>) + 'static,
) {
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), KeyCheckError>>();
    std::thread::spawn(move || {
        let outcome = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt.block_on(key_check::check(provider, key)),
            Err(e) => Err(KeyCheckError::Unreachable(e.to_string())),
        };
        let _ = tx.send(outcome);
    });

    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok(result) => {
                on_result(result);
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            // Worker dropped the sender without a result (panic / unexpected
            // exit); fold it into the unreachable path so the user isn't blocked.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                on_result(Err(KeyCheckError::Unreachable(
                    "verification worker exited without a result".to_owned(),
                )));
                glib::ControlFlow::Break
            }
        }
    });
}
