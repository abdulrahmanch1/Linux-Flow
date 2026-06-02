use std::path::PathBuf;

use evdev::{Device, EventStream, EventType, InputEventKind, Key};
use futures_util::StreamExt;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::hotkey::{HotkeyError, HotkeyEvent};

fn name_to_key(name: &str) -> Option<Key> {
    match name {
        "KEY_RIGHTCTRL" => Some(Key::KEY_RIGHTCTRL),
        "KEY_LEFTCTRL" => Some(Key::KEY_LEFTCTRL),
        "KEY_RIGHTALT" => Some(Key::KEY_RIGHTALT),
        "KEY_LEFTALT" => Some(Key::KEY_LEFTALT),
        "KEY_RIGHTMETA" => Some(Key::KEY_RIGHTMETA),
        "KEY_LEFTMETA" => Some(Key::KEY_LEFTMETA),
        "KEY_CAPSLOCK" => Some(Key::KEY_CAPSLOCK),
        "KEY_RIGHTSHIFT" => Some(Key::KEY_RIGHTSHIFT),
        "KEY_LEFTSHIFT" => Some(Key::KEY_LEFTSHIFT),
        "KEY_F13" => Some(Key::KEY_F13),
        "KEY_F14" => Some(Key::KEY_F14),
        "KEY_F15" => Some(Key::KEY_F15),
        "KEY_F16" => Some(Key::KEY_F16),
        "KEY_F17" => Some(Key::KEY_F17),
        "KEY_F18" => Some(Key::KEY_F18),
        "KEY_F19" => Some(Key::KEY_F19),
        "KEY_F20" => Some(Key::KEY_F20),
        "KEY_F21" => Some(Key::KEY_F21),
        "KEY_F22" => Some(Key::KEY_F22),
        "KEY_F23" => Some(Key::KEY_F23),
        "KEY_F24" => Some(Key::KEY_F24),
        _ => None,
    }
}

fn open_keyboards(key: Key) -> Vec<EventStream> {
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

async fn read_device(mut stream: EventStream, key: Key, tx: mpsc::Sender<HotkeyEvent>) {
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

fn spawn_inotify(hotplug_tx: mpsc::Sender<PathBuf>) {
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
        for event in sync_rx {
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
    });
}

pub struct EvdevBackend;

impl EvdevBackend {
    pub async fn run(tx: mpsc::Sender<HotkeyEvent>, hotkey: &str) -> Result<(), HotkeyError> {
        let key = name_to_key(hotkey).ok_or(HotkeyError::EvdevUnsupported)?;
        let streams = open_keyboards(key);
        if streams.is_empty() {
            return Err(HotkeyError::EvdevUnsupported);
        }

        let (key_tx, mut key_rx) = mpsc::channel::<HotkeyEvent>(16);
        for stream in streams {
            tokio::spawn(read_device(stream, key, key_tx.clone()));
        }

        let (hotplug_tx, mut hotplug_rx) = mpsc::channel::<PathBuf>(8);
        spawn_inotify(hotplug_tx);

        loop {
            tokio::select! {
                Some(ev) = key_rx.recv() => {
                    if tx.send(ev).await.is_err() {
                        return Err(HotkeyError::StreamClosed);
                    }
                }
                Some(path) = hotplug_rx.recv() => {
                    if let Ok(dev) = Device::open(&path) {
                        if dev.supported_keys().is_some_and(|k| k.contains(key)) {
                            if let Ok(stream) = dev.into_event_stream() {
                                tokio::spawn(read_device(stream, key, key_tx.clone()));
                            }
                        }
                    }
                }
            }
        }
    }
}
