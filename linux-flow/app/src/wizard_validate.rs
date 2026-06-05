use zeroize::Zeroizing;

use crate::gtk_async;
use crate::key_check::{self, KeyCheckError, Provider};

/// Runs a key check off the GTK thread and delivers the verdict back on it, so
/// the UI never blocks while the network round-trip is in flight. The key is
/// moved into the worker thread and zeroized there (on drop and inside
/// `key_check::check`).
pub(crate) fn spawn_check(
    provider: Provider,
    key: Zeroizing<String>,
    on_result: impl FnOnce(Result<(), KeyCheckError>) + 'static,
) {
    gtk_async::run_worker(
        move || {
            gtk_async::block_on_new_rt(key_check::check(provider, key), |e| {
                Err(KeyCheckError::Unreachable(e))
            })
        },
        on_result,
    );
}
