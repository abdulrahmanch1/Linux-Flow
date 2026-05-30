mod asr;
mod audio;
mod hotkey;
mod inject;
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
mod wizard_hotkey_wayland;
mod wizard_hotkey_x11;
mod wizard_polkit;

use std::{process::ExitCode, sync::mpsc, time::Duration};

use gtk4::{glib, prelude::*};
use libadwaita::{self as adw, prelude::AdwDialogExt};
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

/// Returns an open File that holds an exclusive flock for the duration of the
/// process, or None if another instance already holds it.
fn acquire_instance_lock() -> Option<std::fs::File> {
    use std::os::unix::io::AsRawFd;
    // SAFETY: getuid() always succeeds on Linux.
    let uid = unsafe { libc::getuid() };
    let path = format!("/run/user/{uid}/linux-flow.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .ok()?;
    // SAFETY: fd is valid; LOCK_EX|LOCK_NB is a valid flock(2) combination.
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret == 0 { Some(file) } else { None }
}

fn main() -> ExitCode {
    let _lock = match acquire_instance_lock() {
        Some(f) => f,
        None => {
            eprintln!("linux-flow: already running — exiting");
            return ExitCode::SUCCESS;
        }
    };
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
    if rt.block_on(wizard::needs_setup()) {
        run_wizard();
    }
    let main_loop = glib::MainLoop::new(None, false);
    if let Err(e) = start_backend(&rt, &main_loop) {
        eprintln!("linux-flow: {e}");
        tracing::error!("{e}");
        // Exit directly rather than returning: start_backend may have already
        // launched the hotkey backend (with its blocking tasks), and returning
        // here would deadlock Runtime::drop the same way Quit does — leaving the
        // process alive holding the instance flock.
        std::process::exit(1);
    }
    main_loop.run();
    // Deliberately bypass Runtime::drop. On Quit, hotkey spawn_blocking tasks
    // (evdev inotify / XGrabKey) can stay blocked indefinitely, which deadlocks
    // the runtime's drop and leaves the process alive holding the instance flock
    // — so a relaunch from the app menu silently exits as "already running".
    // Exiting directly kills every thread and frees the lock immediately.
    std::process::exit(0);
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
    let (about_tx, about_rx) = mpsc::sync_channel::<()>(1);
    let (gtk_quit_tx, gtk_quit_rx) = mpsc::sync_channel::<()>(1);

    let tray = rt.block_on(tray::spawn_tray(quit_tx, settings_tx, about_tx))?;
    let hotkey = {
        let _g = rt.enter();
        hotkey::HotkeyManager::start(&config.general.hotkey)
    }?;
    let mut pipeline = rt.block_on(pipeline::Pipeline::new(config))?;

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
        if about_rx.try_recv().is_ok() {
            adw::AboutDialog::builder()
                .application_name("linux-flow")
                .version(env!("CARGO_PKG_VERSION"))
                .license_type(gtk4::License::Gpl30)
                .website("https://github.com/Abdulrahmanch1/linux-flow")
                .build()
                .present(None::<&gtk4::Widget>);
        }
        glib::ControlFlow::Continue
    });

    Ok(())
}
