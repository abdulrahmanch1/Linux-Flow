use crate::{
    asr::{self, AsrError},
    audio::AudioCapture,
    hotkey::{HotkeyEvent, HotkeyManager},
    ipc_client,
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
}

pub struct Pipeline {
    config: Config,
    llm: LlmClient,
}

impl Pipeline {
    pub fn new(config: Config) -> Result<Self, PipelineError> {
        let llm = LlmClient::new().map_err(|e| PipelineError::LlmInit(e.to_string()))?;
        Ok(Self { config, llm })
    }

    pub async fn run(&self, mut hotkey: HotkeyManager, events: mpsc::Sender<PipelineEvent>) {
        loop {
            if !matches!(hotkey.next().await, Some(HotkeyEvent::Pressed)) { break; }

            let mut audio = AudioCapture::new();
            let pcm_rx = match audio.start() {
                Ok(rx) => rx,
                Err(e) => {
                    let _ = events.send(PipelineEvent::Error(format!("Audio capture failed: {e}"))).await;
                    continue;
                }
            };
            let _ = events.send(PipelineEvent::RecordingStarted).await;

            let soniox_key = match secrets::resolve_key("SONIOX_API_KEY", "soniox").await {
                Ok(k) => k,
                Err(_) => {
                    tokio::task::spawn_blocking(move || audio.stop()).await.ok();
                    let _ = events.send(PipelineEvent::Error(
                        "API key not configured. Open Settings to add it.".into(),
                    )).await;
                    continue;
                }
            };

            let language = self.config.soniox.language.clone();
            let asr = tokio::spawn(async move { asr::transcribe(soniox_key, &language, pcm_rx).await });

            loop {
                match hotkey.next().await {
                    Some(HotkeyEvent::Released) => break,
                    Some(HotkeyEvent::Pressed) => {}
                    None => return,
                }
            }
            tokio::task::spawn_blocking(move || audio.stop()).await.ok();
            let _ = events.send(PipelineEvent::ProcessingStarted).await;

            let asr_result = asr.await
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

            let or_key = match secrets::resolve_key("OPENROUTER_API_KEY", "openrouter").await {
                Ok(k) => k,
                Err(_) => {
                    raw.zeroize();
                    let _ = events.send(PipelineEvent::Error(
                        "API key not configured. Open Settings to add it.".into(),
                    )).await;
                    continue;
                }
            };

            let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| self.config.llm.model.clone());
            let llm_res = self.llm.rewrite(or_key, &model, &raw).await;
            let (mut text, fallback) = match llm_res {
                Ok(t) => (Zeroizing::new(t), false),
                Err(LlmError::EmptyResponse) => (Zeroizing::new((*raw).clone()), true),
                Err(LlmError::Unauthorized) => {
                    raw.zeroize();
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

            match ipc_client::inject_text(text.as_str()).await {
                Ok(()) => { let _ = events.send(PipelineEvent::TextInjected).await; }
                Err(msg) => { let _ = events.send(PipelineEvent::Error(msg)).await; }
            }
            text.zeroize();
        }
    }
}
