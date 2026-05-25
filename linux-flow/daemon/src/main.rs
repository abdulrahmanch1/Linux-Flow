mod clipboard;
mod error;
mod keys;
mod paste_trigger;
mod server;
mod uinput;
mod uinput_inner;
mod wayland_vk;

use std::sync::Arc;

use tokio::signal::unix::{signal, SignalKind};

#[tokio::main]
async fn main() -> Result<(), error::DaemonError> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let mut sigterm = signal(SignalKind::terminate())
        .map_err(|e| error::DaemonError::SocketPath(format!("SIGTERM handler: {e}")))?;
    let mut sigint = signal(SignalKind::interrupt())
        .map_err(|e| error::DaemonError::SocketPath(format!("SIGINT handler: {e}")))?;

    tokio::spawn(async move {
        tokio::select! {
            _ = sigterm.recv() => {}
            _ = sigint.recv() => {}
        }
        let _ = shutdown_tx.send(());
    });

    let injector: Arc<dyn server::KeyInjector> = match uinput::UinputInjector::new() {
        Ok(inj) => Arc::new(inj),
        Err(e) => {
            eprintln!("linux-flow-helper: failed to open /dev/uinput: {e}");
            return Err(e);
        }
    };

    match server::run_server(shutdown_rx, injector).await {
        Err(error::DaemonError::AlreadyRunning) => {
            eprintln!("linux-flow-helper: already running — exiting");
            Ok(())
        }
        r => r,
    }
}
