use evdev::{Device, EventStream, EventType, InputEventKind, Key};
use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::hotkey::{HotkeyError, HotkeyEvent};

fn name_to_key(name: &str) -> Option<Key> {
    match name {
        "KEY_RIGHTCTRL"  => Some(Key::KEY_RIGHTCTRL),
        "KEY_LEFTCTRL"   => Some(Key::KEY_LEFTCTRL),
        "KEY_RIGHTALT"   => Some(Key::KEY_RIGHTALT),
        "KEY_LEFTALT"    => Some(Key::KEY_LEFTALT),
        "KEY_RIGHTMETA"  => Some(Key::KEY_RIGHTMETA),
        "KEY_LEFTMETA"   => Some(Key::KEY_LEFTMETA),
        "KEY_CAPSLOCK"   => Some(Key::KEY_CAPSLOCK),
        "KEY_RIGHTSHIFT" => Some(Key::KEY_RIGHTSHIFT),
        "KEY_LEFTSHIFT"  => Some(Key::KEY_LEFTSHIFT),
        _ => None,
    }
}

fn open_keyboards(key: Key) -> Vec<EventStream> {
    let Ok(paths) = std::fs::read_dir("/dev/input") else { return vec![] };
    paths
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("event"))
        .filter_map(|e| Device::open(e.path()).ok())
        .filter(|d| d.supported_keys().is_some_and(|k| k.contains(key)))
        .filter_map(|d| d.into_event_stream().ok())
        .collect()
}

pub struct EvdevBackend;

impl EvdevBackend {
    pub async fn run(tx: mpsc::Sender<HotkeyEvent>, hotkey: &str) -> Result<(), HotkeyError> {
        let key = name_to_key(hotkey).ok_or(HotkeyError::EvdevUnsupported)?;
        let streams = open_keyboards(key);
        if streams.is_empty() {
            return Err(HotkeyError::EvdevUnsupported);
        }
        let mut merged = futures_util::stream::select_all(streams);
        while let Some(Ok(ev)) = merged.next().await {
            if ev.event_type() != EventType::KEY {
                continue;
            }
            let InputEventKind::Key(k) = ev.kind() else { continue };
            if k != key {
                continue;
            }
            let hk = match ev.value() {
                1 => HotkeyEvent::Pressed,
                0 => HotkeyEvent::Released,
                _ => continue,
            };
            if tx.send(hk).await.is_err() {
                return Err(HotkeyError::StreamClosed);
            }
        }
        Err(HotkeyError::StreamClosed)
    }
}
