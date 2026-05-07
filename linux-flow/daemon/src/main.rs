mod clipboard;
mod error;
mod keymap;
mod keys;
mod server;
mod uinput;

use std::sync::Arc;

use tokio::signal::unix::{signal, SignalKind};

#[tokio::main]
async fn main() -> Result<(), error::DaemonError> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let mut sigterm = signal(SignalKind::terminate())
        .map_err(|e| error::DaemonError::SocketPath(format!("SIGTERM handler: {e}")))?;

    tokio::spawn(async move {
        sigterm.recv().await;
        let _ = shutdown_tx.send(());
    });

    let injector: Arc<dyn server::KeyInjector> = match uinput::UinputInjector::new() {
        Ok(inj) => Arc::new(inj),
        Err(e) => {
            eprintln!("linux-flow-helper: failed to open /dev/uinput: {e}");
            return Err(e);
        }
    };

    server::run_server(shutdown_rx, injector).await
}
