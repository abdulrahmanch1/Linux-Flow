mod asr;
mod audio;
mod hotkey;
mod ipc_client;
mod llm;
mod notifications;
mod pipeline;
mod secrets;
mod settings;
mod settings_hotkey;
mod tray;
mod ui;
mod wizard;
mod wizard_hotkey;
mod wizard_polkit;

use gtk4::{glib, prelude::*};
use linux_flow_shared::config::Config;
use notifications::Notifier;
use pipeline::PipelineEvent;
use tokio::sync::mpsc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let debug = std::env::args().any(|a| a == "--debug");
    setup_logging(debug);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let config = Config::load().unwrap_or_default();
    let setup_needed = rt.block_on(wizard::needs_setup());
    let (quit_tx, mut quit_rx) = mpsc::channel::<()>(1);
    let (ev_tx, ev_rx) = mpsc::channel::<PipelineEvent>(16);
    let tray = rt.block_on(tray::spawn_tray(quit_tx))?;

    gtk4::init()?;
    libadwaita::init()?;

    let notifier = Notifier::new(tray);
    let _guard = rt.enter();
    tokio::spawn(notifier.run(ev_rx));

    if setup_needed {
        let tx = ev_tx;
        wizard::build_wizard(move || {
            start_pipeline(Config::load().unwrap_or_default(), tx.clone());
        })
        .present();
    } else {
        start_pipeline(config, ev_tx);
    }

    let ml = glib::MainLoop::new(None, false);
    let ml2 = ml.clone();
    tokio::spawn(async move {
        let _ = quit_rx.recv().await;
        glib::idle_add_once(move || ml2.quit());
    });

    ml.run();
    Ok(())
}

fn start_pipeline(config: Config, ev_tx: mpsc::Sender<PipelineEvent>) {
    let hotkey = match hotkey::HotkeyManager::start() {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("hotkey: {e}");
            return;
        }
    };
    match pipeline::Pipeline::new(config) {
        Ok(p) => {
            tokio::spawn(async move { p.run(hotkey, ev_tx).await });
        }
        Err(e) => tracing::error!("pipeline init: {e}"),
    }
}

fn setup_logging(debug: bool) {
    if !debug {
        return;
    }
    if let Ok(layer) = tracing_journald::layer() {
        use tracing_subscriber::prelude::*;
        let _ = tracing_subscriber::registry().with(layer).try_init();
    }
}
