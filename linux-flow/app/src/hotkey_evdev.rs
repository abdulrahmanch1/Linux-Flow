#[path = "hotkey_evdev_hotplug.rs"]
mod hotplug;

use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use evdev::{Device, Key};
use tokio::sync::mpsc;

use crate::hotkey::{HotkeyError, HotkeyEvent};

const STOP_POLL: Duration = Duration::from_millis(100);

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

pub struct EvdevBackend;

impl EvdevBackend {
    pub async fn run(
        tx: mpsc::Sender<HotkeyEvent>,
        hotkey: &str,
        stop: Arc<AtomicBool>,
    ) -> Result<(), HotkeyError> {
        let key = name_to_key(hotkey).ok_or(HotkeyError::EvdevUnsupported)?;
        let streams = hotplug::open_keyboards(key);
        if streams.is_empty() {
            return Err(HotkeyError::EvdevUnsupported);
        }

        let (key_tx, mut key_rx) = mpsc::channel::<HotkeyEvent>(16);
        for stream in streams {
            tokio::spawn(hotplug::read_device(stream, key, key_tx.clone()));
        }

        let (hotplug_tx, mut hotplug_rx) = mpsc::channel::<PathBuf>(8);
        let inotify = hotplug::spawn_inotify(hotplug_tx, Arc::clone(&stop));

        let result = loop {
            tokio::select! {
                Some(ev) = key_rx.recv() => {
                    if tx.send(ev).await.is_err() {
                        break Err(HotkeyError::StreamClosed);
                    }
                }
                Some(path) = hotplug_rx.recv() => {
                    if let Ok(dev) = Device::open(&path) {
                        if dev.supported_keys().is_some_and(|k| k.contains(key)) {
                            if let Ok(stream) = dev.into_event_stream() {
                                tokio::spawn(hotplug::read_device(stream, key, key_tx.clone()));
                            }
                        }
                    }
                }
                // Poll the stop flag for a live reload. Dropping `key_tx`/`key_rx`
                // on exit ends every `read_device` task (their `send` errors),
                // closing the evdev devices.
                _ = tokio::time::sleep(STOP_POLL) => {
                    if stop.load(Ordering::Relaxed) {
                        break Ok(());
                    }
                }
            }
        };

        // Confirm the inotify thread (and its fd) is gone before returning, so a
        // reload never accumulates blocking threads.
        if tokio::time::timeout(Duration::from_secs(2), inotify)
            .await
            .is_err()
        {
            tracing::warn!("evdev: inotify teardown timed out");
        }
        result
    }
}
