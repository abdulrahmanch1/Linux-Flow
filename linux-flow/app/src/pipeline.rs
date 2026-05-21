use crate::{
    asr::{self, AsrError},
    audio::AudioCapture,
    hotkey::{HotkeyEvent, HotkeyManager},
    inject::inject_text,
    llm::{LlmClient, LlmError},
    secrets,
};
use linux_flow_shared::config::Config;
use thiserror::Error;
use tokio::sync::mpsc;
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug, Clone)]
pub enum PipelineEvent {
    RecordingStarted,
    ProcessingStarted,
    TextInjected,
    Error(String),
    LlmFallback,
}

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("LLM client init: {0}")]
    LlmInit(String),
    #[error("API key load: {0}")]
    KeyLoad(String),
}

pub struct Pipeline {
    config: Config,
    llm: LlmClient,
    soniox_key: Zeroizing<String>,
    or_key: Zeroizing<String>,
}

impl Pipeline {
    pub async fn new(config: Config) -> Result<Self, PipelineError> {
        let llm = LlmClient::new().map_err(|e| PipelineError::LlmInit(e.to_string()))?;
        let soniox_key = secrets::resolve_key("SONIOX_API_KEY", "soniox").await
            .map_err(|e| PipelineError::KeyLoad(e.to_string()))?;
        let or_key = secrets::resolve_key("OPENROUTER_API_KEY", "openrouter").await
            .map_err(|e| PipelineError::KeyLoad(e.to_string()))?;
        Ok(Self { config, llm, soniox_key, or_key })
    }

    pub async fn run(&mut self, mut hotkey: HotkeyManager, events: mpsc::Sender<PipelineEvent>) {
        loop {
            if !matches!(hotkey.next().await, Some(HotkeyEvent::Pressed)) { break; }

            if let Ok(fresh) = Config::load() { self.config = fresh; }

            let mut audio = AudioCapture::new();
            let (pcm_rx, audio_err_rx) = match audio.start() {
                Ok(pair) => pair,
                Err(e) => {
                    let _ = events.send(PipelineEvent::Error(format!("Audio capture failed: {e}"))).await;
                    continue;
                }
            };
            let _ = events.send(PipelineEvent::RecordingStarted).await;

            let soniox_key = self.soniox_key.clone();
            let language = self.config.soniox.language.clone();
            let asr = tokio::spawn(async move { asr::transcribe(soniox_key, &language, pcm_rx).await });

            let released = loop {
                match hotkey.next().await {
                    Some(HotkeyEvent::Released) => break true,
                    Some(HotkeyEvent::Pressed) => {}
                    None => break false,
                }
            };
            tokio::task::spawn_blocking(move || audio.stop()).await.ok();
            if !released { asr.abort(); return; }
            if let Ok(e) = audio_err_rx.try_recv() {
                let _ = events.send(PipelineEvent::Error(format!("Audio capture failed: {e}"))).await;
                continue;
            }
            let _ = events.send(PipelineEvent::ProcessingStarted).await;

            let asr_joined = asr.await;
            if let Ok(Err(AsrError::Unauthorized)) = &asr_joined {
                let _ = secrets::resolve_key("SONIOX_API_KEY", "soniox").await
                    .map(|k| self.soniox_key = k);
            }
            let asr_result = asr_joined
                .map_err(|_| "Speech recognition failed. Please try again.".to_owned())
                .and_then(|r| r.map_err(|e| match e {
                    AsrError::ConnectTimeout | AsrError::StreamTimeout =>
                        "Could not connect to Soniox. Check your connection and API key.".to_owned(),
                    AsrError::Unauthorized =>
                        "Soniox API key is invalid. Open Settings to update it.".to_owned(),
                    _ => "Speech recognition failed. Please try again.".to_owned(),
                }));
            let mut raw = match asr_result {
                Ok(t) => t,
                Err(msg) => { let _ = events.send(PipelineEvent::Error(msg)).await; continue; }
            };

            let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| self.config.llm.model.clone());
            let llm_res = self.llm.rewrite(self.or_key.clone(), &model, &raw).await;
            let (mut text, fallback) = match llm_res {
                Ok(t) => (Zeroizing::new(t), false),
                Err(LlmError::EmptyResponse) => (std::mem::replace(&mut raw, Zeroizing::new(String::new())), true),
                Err(LlmError::Unauthorized) => {
                    raw.zeroize();
                    let _ = secrets::resolve_key("OPENROUTER_API_KEY", "openrouter").await
                        .map(|k| self.or_key = k);
                    let _ = events.send(PipelineEvent::Error(
                        "OpenRouter API key is invalid. Open Settings to update it.".into(),
                    )).await;
                    continue;
                }
                Err(_) => {
                    raw.zeroize();
                    let _ = events.send(PipelineEvent::Error(
                        "Could not reach OpenRouter. Check your connection.".into(),
                    )).await;
                    continue;
                }
            };
            raw.zeroize();
            if fallback { let _ = events.send(PipelineEvent::LlmFallback).await; }

            match inject_text(text.as_str()).await {
                Ok(()) => { let _ = events.send(PipelineEvent::TextInjected).await; }
                Err(msg) => { let _ = events.send(PipelineEvent::Error(msg)).await; }
            }
            text.zeroize();
        }
    }
}

