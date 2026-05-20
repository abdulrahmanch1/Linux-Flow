use ksni::{
    menu::{MenuItem, StandardItem},
    Icon, Tray, TrayMethods,
};
use std::{sync::mpsc as stdmpsc, time::Duration};
use thiserror::Error;
use tokio::sync::mpsc;

const ICON_SIZE: usize = 22;
const WAVE_FRAMES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    Idle,
    Recording,
    Processing,
    Error,
}

#[derive(Debug, Error)]
pub enum TrayError {
    #[error("tray service: {0}")]
    Service(String),
}

#[derive(Clone)]
pub struct TrayHandle { inner: ksni::Handle<AppTray> }

impl TrayHandle {
    pub async fn set_state(&self, state: TrayState) {
        self.inner.update(|t| t.state = state).await;
    }
}

struct AppTray {
    state: TrayState,
    frame: usize,
    wave_frames: Vec<Vec<u8>>,
    quit_tx: mpsc::Sender<()>,
    settings_tx: stdmpsc::SyncSender<()>,
    about_tx: stdmpsc::SyncSender<()>,
}

impl Tray for AppTray {
    fn id(&self) -> String { "linux-flow".into() }
    fn title(&self) -> String { "linux-flow".into() }

    fn icon_name(&self) -> String {
        match self.state {
            TrayState::Idle => "audio-input-microphone".into(),
            TrayState::Recording => "media-record".into(),
            TrayState::Processing => "emblem-synchronizing".into(),
            TrayState::Error => "dialog-warning".into(),
        }
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        if self.state != TrayState::Recording { return vec![]; }
        vec![Icon {
            width: ICON_SIZE as i32,
            height: ICON_SIZE as i32,
            data: self.wave_frames[self.frame].clone(),
        }]
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let quit_tx = self.quit_tx.clone();
        let settings_tx = self.settings_tx.clone();
        let about_tx = self.about_tx.clone();
        vec![
            StandardItem { label: "linux-flow".into(), enabled: false, ..Default::default() }.into(),
            MenuItem::Separator,
            StandardItem {
                label: "Settings\u{2026}".into(),
                activate: Box::new(move |_| { let _ = settings_tx.try_send(()); }),
                ..Default::default()
            }.into(),
            StandardItem {
                label: "About".into(),
                activate: Box::new(move |_| { let _ = about_tx.try_send(()); }),
                ..Default::default()
            }.into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(move |_| { let _ = quit_tx.try_send(()); }),
                ..Default::default()
            }.into(),
        ]
    }
}

fn make_wave_frames() -> Vec<Vec<u8>> {
    (0..WAVE_FRAMES)
        .map(|frame| {
            let mut data = vec![0u8; ICON_SIZE * ICON_SIZE * 4];
            for x in 0..ICON_SIZE {
                let phase = (x as f64 / ICON_SIZE as f64 + frame as f64 / WAVE_FRAMES as f64)
                    * 2.0
                    * std::f64::consts::PI;
                let height = ((phase.sin() * 0.35 + 0.55) * ICON_SIZE as f64) as usize;
                for y in (ICON_SIZE - height.min(ICON_SIZE))..ICON_SIZE {
                    let i = (y * ICON_SIZE + x) * 4;
                    data[i..i + 4].copy_from_slice(&[0xFF, 0x4C, 0xAF, 0x50]);
                }
            }
            data
        })
        .collect()
}

pub async fn spawn_tray(
    quit_tx: mpsc::Sender<()>,
    settings_tx: stdmpsc::SyncSender<()>,
    about_tx: stdmpsc::SyncSender<()>,
) -> Result<TrayHandle, TrayError> {
    let handle = AppTray {
        state: TrayState::Idle,
        frame: 0,
        wave_frames: make_wave_frames(),
        quit_tx,
        settings_tx,
        about_tx,
    }
    .spawn()
    .await
    .map_err(|e| TrayError::Service(e.to_string()))?;

    let anim = handle.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            tick.tick().await;
            if anim
                .update(|t| {
                    if t.state == TrayState::Recording {
                        t.frame = (t.frame + 1) % WAVE_FRAMES;
                    }
                })
                .await
                .is_none()
            {
                break;
            }
        }
    });

    Ok(TrayHandle { inner: handle })
}
