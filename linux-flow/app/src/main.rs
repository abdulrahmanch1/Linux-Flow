mod asr;
mod audio;
mod hotkey;
mod llm;
mod notifications;
mod pipeline;
mod secrets;
mod settings;
mod tray;
mod ui;
mod wizard;
mod wizard_complete;
mod wizard_hotkey;
mod wizard_polkit;

use std::{process::ExitCode, sync::mpsc, time::Duration};

use gtk4::{glib, prelude::*};
use libadwaita as adw;
use linux_flow_shared::config::Config;
use thiserror::Error;
use tokio::runtime::Runtime;

#[derive(Debug, Error)]
enum AppError {
    #[error("tray: {0}")]
    Tray(#[from] tray::TrayError),
    #[error("hotkey: {0}")]
    Hotkey(#[from] hotkey::HotkeyError),
    #[error("pipeline: {0}")]
    Pipeline(#[from] pipeline::PipelineError),
}

fn main() -> ExitCode {
    let debug = std::env::args().any(|a| a == "--debug");
    init_tracing(debug);
    if adw::init().is_err() {
        eprintln!("failed to initialize libadwaita");
        return ExitCode::FAILURE;
    }
    let rt = match Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("tokio runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    if wizard::needs_setup() {
        run_wizard();
    }
    let main_loop = glib::MainLoop::new(None, false);
    if let Err(e) = start_backend(&rt, &main_loop) {
        tracing::error!("{e}");
        return ExitCode::FAILURE;
    }
    main_loop.run();
    ExitCode::SUCCESS
}

fn init_tracing(debug: bool) {
    if !debug {
        return;
    }
    use tracing_subscriber::prelude::*;
    match tracing_journald::layer() {
        Ok(layer) => {
            tracing_subscriber::registry().with(layer).init();
        }
        Err(e) => eprintln!("journald unavailable: {e}"),
    }
}

fn run_wizard() {
    let ml = glib::MainLoop::new(None, false);
    let win = wizard::build_wizard();
    let ml_quit = ml.clone();
    win.connect_destroy(move |_| ml_quit.quit());
    win.present();
    ml.run();
}

fn start_backend(rt: &Runtime, main_loop: &glib::MainLoop) -> Result<(), AppError> {
    let config = Config::load().unwrap_or_default();
    let (events_tx, events_rx) = tokio::sync::mpsc::channel(16);
    let (quit_tx, mut quit_rx) = tokio::sync::mpsc::channel::<()>(1);
    let (settings_tx, settings_rx) = mpsc::sync_channel::<()>(1);
    let (gtk_quit_tx, gtk_quit_rx) = mpsc::sync_channel::<()>(1);

    let tray = rt.block_on(tray::spawn_tray(quit_tx, settings_tx))?;
    let hotkey = {
        let _g = rt.enter();
        hotkey::HotkeyManager::start()
    }?;
    let pipeline = pipeline::Pipeline::new(config)?;

    rt.spawn(notifications::handle_events(events_rx, tray, None));
    rt.spawn(async move { pipeline.run(hotkey, events_tx).await });
    rt.spawn(async move {
        quit_rx.recv().await;
        let _ = gtk_quit_tx.send(());
    });

    let ml = main_loop.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        if gtk_quit_rx.try_recv().is_ok() {
            ml.quit();
            return glib::ControlFlow::Break;
        }
        if settings_rx.try_recv().is_ok() {
            settings::build_settings().present();
        }
        glib::ControlFlow::Continue
    });

    Ok(())
}
