use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use evdev::{Device, EventStream, EventType, InputEventKind, Key};
use futures_util::StreamExt;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::hotkey::HotkeyEvent;

const STOP_POLL: Duration = Duration::from_millis(200);

pub(crate) fn open_keyboards(key: Key) -> Vec<EventStream> {
    let Ok(paths) = std::fs::read_dir("/dev/input") else {
        return vec![];
    };
    paths
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("event"))
        .filter_map(|e| Device::open(e.path()).ok())
        .filter(|d| d.supported_keys().is_some_and(|k| k.contains(key)))
        .filter_map(|d| d.into_event_stream().ok())
        .collect()
}

pub(crate) async fn read_device(mut stream: EventStream, key: Key, tx: mpsc::Sender<HotkeyEvent>) {
    while let Some(Ok(ev)) = stream.next().await {
        if ev.event_type() != EventType::KEY {
            continue;
        }
        let InputEventKind::Key(k) = ev.kind() else {
            continue;
        };
        if k != key {
            continue;
        }
        let hk = match ev.value() {
            1 => HotkeyEvent::Pressed,
            0 => HotkeyEvent::Released,
            _ => continue,
        };
        if tx.send(hk).await.is_err() {
            return;
        }
    }
    // Stream ended (device disconnected) — task exits; inotify handles reconnect.
}

/// Watches `/dev/input` for newly created event nodes (keyboard hotplug) and
/// forwards their paths. Returns a handle the parent awaits during teardown so
/// the inotify thread (and its fd) is confirmed gone before a reload.
pub(crate) fn spawn_inotify(
    hotplug_tx: mpsc::Sender<PathBuf>,
    stop: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        let (sync_tx, sync_rx) = std::sync::mpsc::channel();
        let mut watcher = match RecommendedWatcher::new(sync_tx, notify::Config::default()) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("inotify watcher failed to initialise: {e}");
                return;
            }
        };
        if let Err(e) = watcher.watch(
            std::path::Path::new("/dev/input"),
            RecursiveMode::NonRecursive,
        ) {
            tracing::warn!("inotify watch on /dev/input failed: {e}");
            return;
        }
        // Poll with a timeout instead of an unbounded `for event in sync_rx` so
        // the stop flag is observed and the watcher (and its inotify fd) is
        // dropped on teardown — an unbounded blocking recv would never die on
        // abort and would leak the thread on every reload.
        loop {
            match sync_rx.recv_timeout(STOP_POLL) {
                Ok(event) => {
                    let Ok(ev) = event else { continue };
                    if !matches!(ev.kind, EventKind::Create(_)) {
                        continue;
                    }
                    for path in ev.paths {
                        if path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with("event"))
                        {
                            let _ = hotplug_tx.blocking_send(path);
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    })
}
