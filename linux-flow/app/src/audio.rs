use pipewire::{
    context::Context,
    main_loop::MainLoop,
    properties::properties,
    spa,
    spa::{
        param::{
            audio::AudioFormat,
            format::{FormatProperties, MediaSubtype, MediaType},
            ParamType,
        },
        pod::{serialize::PodSerializer, Object, Property, PropertyFlags, Value},
        utils::{Id, SpaTypes},
    },
    stream::{Stream, StreamFlags},
};
use std::{
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

const SAMPLE_RATE: i32 = 16_000;
const CHANNELS: i32 = 1;

type StartResult = (
    mpsc::Receiver<Vec<u8>>,
    std::sync::mpsc::Receiver<AudioError>,
    oneshot::Receiver<()>,
);

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("PipeWire error: {0}")]
    PipeWire(#[from] pipewire::Error),
    #[error("audio POD error: {0}")]
    Pod(String),
}

pub struct AudioCapture {
    stop_flag: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl AudioCapture {
    pub fn new() -> Self {
        Self {
            stop_flag: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }

    pub fn start(&mut self) -> Result<StartResult, AudioError> {
        self.stop_flag.store(false, Ordering::Release);
        let stop = Arc::clone(&self.stop_flag);
        let (pcm_tx, pcm_rx) = mpsc::channel(512);
        let (err_tx, err_rx) = std::sync::mpsc::channel::<AudioError>();
        let (ready_tx, ready_rx) = oneshot::channel::<()>();
        let handle = thread::spawn(move || {
            if let Err(e) = capture_loop(pcm_tx, stop, ready_tx) {
                let _ = err_tx.send(e);
            }
        });
        self.thread = Some(handle);
        Ok((pcm_rx, err_rx, ready_rx))
    }

    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

fn audio_params_pod() -> Result<Vec<u8>, AudioError> {
    let obj = Value::Object(Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties: vec![
            Property {
                key: FormatProperties::MediaType.as_raw(),
                flags: PropertyFlags::empty(),
                value: Value::Id(Id(MediaType::Audio.as_raw())),
            },
            Property {
                key: FormatProperties::MediaSubtype.as_raw(),
                flags: PropertyFlags::empty(),
                value: Value::Id(Id(MediaSubtype::Raw.as_raw())),
            },
            Property {
                key: FormatProperties::AudioFormat.as_raw(),
                flags: PropertyFlags::empty(),
                value: Value::Id(Id(AudioFormat::S16LE.as_raw())),
            },
            Property {
                key: FormatProperties::AudioRate.as_raw(),
                flags: PropertyFlags::empty(),
                value: Value::Int(SAMPLE_RATE),
            },
            Property {
                key: FormatProperties::AudioChannels.as_raw(),
                flags: PropertyFlags::empty(),
                value: Value::Int(CHANNELS),
            },
        ],
    });
    let (w, _) = PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &obj)
        .map_err(|e| AudioError::Pod(e.to_string()))?;
    Ok(w.into_inner())
}

fn capture_loop(
    pcm_tx: mpsc::Sender<Vec<u8>>,
    stop: Arc<AtomicBool>,
    ready_tx: oneshot::Sender<()>,
) -> Result<(), AudioError> {
    pipewire::init();
    let mainloop = Rc::new(MainLoop::new(None)?);
    let context = Context::new(&*mainloop)?;
    let core = context.connect(None)?;
    let stream = Stream::new(
        &core,
        "linux-flow-capture",
        properties! {
            *pipewire::keys::MEDIA_TYPE => "Audio",
            *pipewire::keys::MEDIA_CATEGORY => "Capture",
            *pipewire::keys::MEDIA_ROLE => "Communication",
        },
    )?;
    let ml = Rc::clone(&mainloop);
    let _listener = stream
        .add_local_listener_with_user_data((pcm_tx, stop, ml, Some(ready_tx)))
        .process(|stream, (tx, stop, ml, ready)| {
            if stop.load(Ordering::Acquire) {
                ml.quit();
                return;
            }
            if let Some(s) = ready.take() {
                let _ = s.send(());
            }
            if let Some(mut buf) = stream.dequeue_buffer() {
                if let Some(d) = buf.datas_mut().first_mut() {
                    let sz = d.chunk().size() as usize;
                    if sz > 0 {
                        if let Some(raw) = d.data() {
                            let _ = tx.blocking_send(raw[..sz].to_vec());
                        }
                    }
                }
            }
        })
        .register()?;
    let params_buf = audio_params_pod()?;
    let params_pod = pipewire::spa::pod::Pod::from_bytes(&params_buf)
        .ok_or_else(|| AudioError::Pod("invalid audio POD bytes".into()))?;
    stream.connect(
        spa::utils::Direction::Input,
        None,
        StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
        &mut [params_pod],
    )?;
    mainloop.run();
    stream.disconnect().map_err(AudioError::PipeWire)?;
    Ok(())
}
