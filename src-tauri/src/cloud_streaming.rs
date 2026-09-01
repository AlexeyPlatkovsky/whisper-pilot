//! Provider-neutral parsing and PCM conversion for Cloud Streaming.
//!
//! Transport ownership deliberately stays outside this module: these pure
//! functions are shared by each WebSocket adapter and make provider payloads
//! testable without a network connection or a credential.

use crate::cloud_provider::CloudProvider;
use crate::error::{AppError, Result};
use crate::streaming_audio::resample_linear;
use crate::transcribe::UNDETECTED_LANGUAGE;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    http::{header::AUTHORIZATION, HeaderValue},
    Message,
};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

const CAPTURE_SAMPLE_RATE: u32 = 16_000;
const OPENAI_SAMPLE_RATE: u32 = 24_000;
const OPENAI_COMMIT_INTERVAL_CAPTURE_SAMPLES: u64 = CAPTURE_SAMPLE_RATE as u64 * 7;
const OPENAI_MODEL_API_BASE_URL: &str = "https://api.openai.com/v1/models";
const OPENAI_TRANSCRIPTION_PROMPT: &str = "A professional meeting with natural pauses that may include names, numbers, acronyms, and technical terms.";

/// Immutable connection details for a single provider session. Credentials are
/// held only long enough to build the HTTPS/WSS Authorization header; callers
/// must not log or serialize this value.
pub struct CloudConnectionSpec {
    pub url: String,
    pub authorization: String,
    pub sample_rate: u32,
    pub initial_messages: Vec<String>,
}

/// The WebSocket frame required by an outbound audio chunk. It intentionally
/// omits `Debug` so user audio cannot be accidentally included in diagnostics.
pub enum CloudOutboundMessage {
    Binary(Vec<u8>),
    Text(String),
}

/// A transient cloud result. Final results are persisted by the Streaming
/// command; partial results are emitted to the UI only and are never stored.
pub enum CloudStreamingResult {
    Partial {
        item_id: Option<String>,
        text: String,
    },
    Final {
        item_id: Option<String>,
        text: String,
        language: String,
        end_ms: i64,
    },
    /// A safe, app-authored error message. Remote provider payloads and
    /// credentials must never be placed in this variant.
    Failed { message: String },
}

/// One authenticated provider socket. Neither this type nor its errors expose
/// the authorization value; callers must keep it scoped to the worker task.
pub struct CloudTransport {
    provider: CloudProvider,
    spec: CloudConnectionSpec,
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    transcript_assembler: CloudTranscriptAssembler,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudTranscriptEvent {
    Partial {
        item_id: Option<String>,
        text: String,
        incremental: bool,
    },
    Final {
        item_id: Option<String>,
        text: String,
        language: String,
    },
}

/// Reconstructs providers' incremental per-turn deltas into a stable partial.
/// OpenAI sends only newly decoded fragments; other initial providers send a
/// complete revised partial and therefore pass through unchanged.
#[derive(Default)]
pub struct CloudTranscriptAssembler {
    partials: HashMap<String, String>,
}

impl CloudTranscriptAssembler {
    pub fn apply(&mut self, event: CloudTranscriptEvent) -> CloudTranscriptEvent {
        match event {
            CloudTranscriptEvent::Partial {
                item_id: Some(item_id),
                text,
                incremental: true,
            } => {
                let partial = self.partials.entry(item_id.clone()).or_default();
                partial.push_str(&text);
                CloudTranscriptEvent::Partial {
                    item_id: Some(item_id),
                    text: partial.clone(),
                    incremental: false,
                }
            }
            CloudTranscriptEvent::Final {
                item_id,
                text,
                language,
            } => {
                if let Some(item_id) = &item_id {
                    self.partials.remove(item_id);
                }
                CloudTranscriptEvent::Final {
                    item_id,
                    text,
                    language,
                }
            }
            event => event,
        }
    }
}

/// A provider response that confirms the authenticated transcription session
/// is usable. OpenAI validates the requested session asynchronously after the
/// WebSocket upgrade, so a successful socket handshake is not enough.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudTransportReadyEvent {
    Ready,
}

/// Builds the documented, provider-specific WebSocket connection contract.
/// API keys are always sent in an Authorization header, never in the URL or
/// any JSON control event.
pub fn connection_spec(provider: CloudProvider, api_key: &str) -> CloudConnectionSpec {
    match provider {
        CloudProvider::Deepgram => CloudConnectionSpec {
            url: format!(
                "wss://api.deepgram.com/v1/listen?model={}&encoding=linear16&sample_rate=16000&channels=1&interim_results=true&smart_format=true&endpointing=300",
                provider.transport_model()
            ),
            authorization: format!("Token {api_key}"),
            sample_rate: CAPTURE_SAMPLE_RATE,
            initial_messages: Vec::new(),
        },
        CloudProvider::AssemblyAi => CloudConnectionSpec {
            url: format!(
                "wss://streaming.assemblyai.com/v3/ws?sample_rate=16000&speech_model={}",
                provider.transport_model()
            ),
            authorization: api_key.to_string(),
            sample_rate: CAPTURE_SAMPLE_RATE,
            initial_messages: Vec::new(),
        },
        CloudProvider::OpenAi => CloudConnectionSpec {
            url: "wss://api.openai.com/v1/realtime?intent=transcription".to_string(),
            authorization: format!("Bearer {api_key}"),
            sample_rate: OPENAI_SAMPLE_RATE,
            initial_messages: vec![serde_json::json!({
                "type": "session.update",
                "session": {
                    "type": "transcription",
                    "audio": {
                        "input": {
                            "format": { "type": "audio/pcm", "rate": OPENAI_SAMPLE_RATE },
                            "transcription": {
                                "model": provider.transport_model(),
                                "delay": "high",
                                "prompt": OPENAI_TRANSCRIPTION_PROMPT
                            },
                            "turn_detection": null
                        }
                    }
                }
            })
            .to_string()],
        },
    }
}

/// Encodes one normalized 16 kHz capture chunk for a provider. Deepgram and
/// AssemblyAI accept raw PCM WebSocket binary frames; OpenAI accepts a JSON
/// Realtime append event carrying base64-encoded 24 kHz PCM.
pub fn outbound_audio_message(
    spec: &CloudConnectionSpec,
    capture_samples: &[f32],
) -> CloudOutboundMessage {
    let samples = if spec.sample_rate == CAPTURE_SAMPLE_RATE {
        capture_samples.to_vec()
    } else {
        resample_linear(capture_samples, CAPTURE_SAMPLE_RATE, spec.sample_rate)
    };
    let pcm = pcm_s16le(&samples);

    if spec.sample_rate == OPENAI_SAMPLE_RATE {
        CloudOutboundMessage::Text(
            serde_json::json!({
                "type": "input_audio_buffer.append",
                "audio": base64::engine::general_purpose::STANDARD.encode(pcm),
            })
            .to_string(),
        )
    } else {
        CloudOutboundMessage::Binary(pcm)
    }
}

/// Creates the explicit turn boundary required by GPT Live Transcribe when
/// server VAD is disabled. Empty buffers are never committed because OpenAI
/// rejects them; non-OpenAI providers own their turn boundaries server-side.
pub fn outbound_commit_message(
    provider: CloudProvider,
    buffered_capture_samples: u64,
    flush: bool,
) -> Option<CloudOutboundMessage> {
    if provider != CloudProvider::OpenAi
        || buffered_capture_samples == 0
        || (!flush && buffered_capture_samples < OPENAI_COMMIT_INTERVAL_CAPTURE_SAMPLES)
    {
        return None;
    }

    Some(CloudOutboundMessage::Text(
        serde_json::json!({ "type": "input_audio_buffer.commit" }).to_string(),
    ))
}

impl CloudTransport {
    /// Establishes TLS and sends required session initialization before audio
    /// capture begins. That ordering guarantees no audio leaves the device
    /// until the selected cloud provider is reachable.
    pub async fn connect(provider: CloudProvider, api_key: &str) -> Result<Self> {
        let spec = connection_spec(provider, api_key);
        let mut request = spec.url.clone().into_client_request().map_err(|_| {
            AppError::Capture("Unable to prepare cloud transcription connection.".to_string())
        })?;
        let authorization = HeaderValue::from_str(&spec.authorization).map_err(|_| {
            AppError::Capture("Unable to prepare cloud transcription connection.".to_string())
        })?;
        request.headers_mut().insert(AUTHORIZATION, authorization);

        let (mut socket, _) = connect_async(request).await.map_err(|_| {
            let message = match provider {
                CloudProvider::OpenAi => {
                    "OpenAI transcription setup could not be established. Confirm this project has access to GPT Live Transcribe."
                }
                CloudProvider::Deepgram | CloudProvider::AssemblyAi => {
                    "Unable to connect to cloud transcription. Check your network and API key."
                }
            };
            AppError::Capture(message.to_string())
        })?;
        for message in &spec.initial_messages {
            socket
                .send(Message::Text(message.clone()))
                .await
                .map_err(|_| {
                    AppError::Capture(
                        "Unable to start cloud transcription. Check your network and API key."
                            .to_string(),
                    )
                })?;
        }

        let mut transport = Self {
            provider,
            spec,
            socket,
            transcript_assembler: CloudTranscriptAssembler::default(),
        };
        transport.wait_until_ready().await?;
        Ok(transport)
    }

    /// Authenticates a candidate key without sending captured audio. This is
    /// deliberately narrower than streaming startup: OpenAI verification checks
    /// authenticated model access, while streaming startup validates the live
    /// session immediately before capture begins.
    pub async fn verify(provider: CloudProvider, api_key: &str) -> Result<()> {
        if provider == CloudProvider::OpenAi {
            return verify_openai_model_access(api_key).await;
        }
        let mut transport = Self::connect(provider, api_key).await?;
        transport.finish().await;
        Ok(())
    }

    async fn wait_until_ready(&mut self) -> Result<()> {
        if self.provider != CloudProvider::OpenAi {
            return Ok(());
        }

        timeout(Duration::from_secs(10), async {
            loop {
                let message = self
                    .socket
                    .next()
                    .await
                    .ok_or_else(|| {
                        AppError::Capture(
                            "Cloud transcription connection closed unexpectedly.".to_string(),
                        )
                    })?
                    .map_err(|_| {
                        AppError::Capture("Cloud transcription connection failed.".to_string())
                    })?;
                match message {
                    Message::Text(payload) => {
                        if matches!(
                            parse_provider_ready_event(self.provider, payload.as_str())?,
                            Some(CloudTransportReadyEvent::Ready)
                        ) {
                            return Ok(());
                        }
                    }
                    Message::Ping(payload) => {
                        self.socket
                            .send(Message::Pong(payload))
                            .await
                            .map_err(|_| {
                                AppError::Capture(
                                    "Cloud transcription connection failed.".to_string(),
                                )
                            })?;
                    }
                    Message::Close(_) => {
                        return Err(AppError::Capture(
                            "Cloud transcription connection closed unexpectedly.".to_string(),
                        ))
                    }
                    Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => {}
                }
            }
        })
        .await
        .map_err(|_| {
            AppError::Capture(
                "Cloud transcription setup timed out. Check your network and API key.".to_string(),
            )
        })?
    }

    /// Pumps capture samples and provider events until the capture channel is
    /// closed. Any network/protocol failure becomes a safe, user-presentable
    /// error; provider payloads and credentials never cross this boundary.
    pub async fn run(
        mut self,
        mut samples_rx: mpsc::Receiver<Vec<f32>>,
        results_tx: mpsc::Sender<CloudStreamingResult>,
    ) -> Result<()> {
        let mut captured_samples: u64 = 0;
        let mut buffered_openai_samples: u64 = 0;
        let mut pending_openai_commits: u64 = 0;

        loop {
            tokio::select! {
                maybe_samples = samples_rx.recv() => {
                    match maybe_samples {
                        Some(samples) => {
                            captured_samples = captured_samples.saturating_add(samples.len() as u64);
                            self.send_audio(&samples).await?;
                            buffered_openai_samples = buffered_openai_samples
                                .saturating_add(samples.len() as u64);
                            if let Some(commit) = outbound_commit_message(
                                self.provider,
                                buffered_openai_samples,
                                false,
                            ) {
                                self.send_outbound_message(commit).await?;
                                buffered_openai_samples = 0;
                                pending_openai_commits = pending_openai_commits.saturating_add(1);
                            }
                        }
                        None => {
                            if let Some(commit) = outbound_commit_message(
                                self.provider,
                                buffered_openai_samples,
                                true,
                            ) {
                                self.send_outbound_message(commit).await?;
                                pending_openai_commits = pending_openai_commits.saturating_add(1);
                            }
                            self.drain_openai_commits(
                                pending_openai_commits,
                                captured_samples,
                                &results_tx,
                            )
                            .await?;
                            self.finish().await;
                            return Ok(());
                        }
                    }
                }
                maybe_message = self.socket.next() => {
                    let message = maybe_message.ok_or_else(|| {
                        AppError::Capture("Cloud transcription connection closed unexpectedly.".to_string())
                    })?.map_err(|_| {
                        AppError::Capture("Cloud transcription connection failed.".to_string())
                    })?;
                    if self.handle_message(message, captured_samples, &results_tx).await?
                        && pending_openai_commits > 0
                    {
                        pending_openai_commits -= 1;
                    }
                }
            }
        }
    }

    async fn send_audio(&mut self, samples: &[f32]) -> Result<()> {
        self.send_outbound_message(outbound_audio_message(&self.spec, samples))
            .await
    }

    async fn send_outbound_message(&mut self, message: CloudOutboundMessage) -> Result<()> {
        let message = match message {
            CloudOutboundMessage::Binary(audio) => Message::Binary(audio),
            CloudOutboundMessage::Text(payload) => Message::Text(payload),
        };
        self.socket
            .send(message)
            .await
            .map_err(|_| AppError::Capture("Cloud transcription connection failed.".to_string()))
    }

    async fn handle_message(
        &mut self,
        message: Message,
        captured_samples: u64,
        results_tx: &mpsc::Sender<CloudStreamingResult>,
    ) -> Result<bool> {
        let mut completed_commit = false;
        match message {
            Message::Text(payload) => {
                completed_commit =
                    provider_event_completes_commit(self.provider, payload.as_str())?;
                let transcript_event = parse_provider_event(self.provider, payload.as_str())?
                    .map(|event| self.transcript_assembler.apply(event));
                match transcript_event {
                    Some(CloudTranscriptEvent::Partial { item_id, text, .. }) => {
                        let _ = results_tx
                            .send(CloudStreamingResult::Partial { item_id, text })
                            .await;
                    }
                    Some(CloudTranscriptEvent::Final {
                        item_id,
                        text,
                        language,
                    }) => {
                        let end_ms = captured_samples
                            .saturating_mul(1_000)
                            .checked_div(CAPTURE_SAMPLE_RATE as u64)
                            .unwrap_or(0)
                            .min(i64::MAX as u64) as i64;
                        let _ = results_tx
                            .send(CloudStreamingResult::Final {
                                item_id,
                                text,
                                language,
                                end_ms,
                            })
                            .await;
                    }
                    None => {}
                }
            }
            Message::Ping(payload) => {
                self.socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|_| {
                        AppError::Capture("Cloud transcription connection failed.".to_string())
                    })?;
            }
            Message::Close(_) => {
                return Err(AppError::Capture(
                    "Cloud transcription connection closed unexpectedly.".to_string(),
                ));
            }
            Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
        Ok(completed_commit)
    }

    async fn drain_openai_commits(
        &mut self,
        mut pending_commits: u64,
        captured_samples: u64,
        results_tx: &mpsc::Sender<CloudStreamingResult>,
    ) -> Result<()> {
        if self.provider != CloudProvider::OpenAi || pending_commits == 0 {
            return Ok(());
        }

        timeout(Duration::from_secs(10), async {
            while pending_commits > 0 {
                let message = self
                    .socket
                    .next()
                    .await
                    .ok_or_else(|| {
                        AppError::Capture(
                            "Cloud transcription connection closed unexpectedly.".to_string(),
                        )
                    })?
                    .map_err(|_| {
                        AppError::Capture("Cloud transcription connection failed.".to_string())
                    })?;
                if self
                    .handle_message(message, captured_samples, results_tx)
                    .await?
                {
                    pending_commits -= 1;
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| AppError::Capture("Cloud transcription finalization timed out.".to_string()))?
    }

    async fn finish(&mut self) {
        let termination = match self.provider {
            CloudProvider::Deepgram => Some(r#"{"type":"CloseStream"}"#),
            CloudProvider::AssemblyAi => Some(r#"{"type":"Terminate"}"#),
            CloudProvider::OpenAi => None,
        };
        if let Some(termination) = termination {
            let _ = self.socket.send(Message::Text(termination.into())).await;
        }
        let _ = self.socket.close(None).await;
    }
}

async fn verify_openai_model_access(api_key: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let model = CloudProvider::OpenAi.transport_model();
    let response = client
        .get(format!("{OPENAI_MODEL_API_BASE_URL}/{model}"))
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|_| {
            AppError::Capture(
                "Unable to connect to OpenAI. Check your network and API key.".to_string(),
            )
        })?;
    if !response.status().is_success() {
        return Err(AppError::Capture(
            "OpenAI API key cannot access GPT Live Transcribe.".to_string(),
        ));
    }
    let response_text = response
        .text()
        .await
        .map_err(|_| AppError::Capture("OpenAI returned an invalid model response.".to_string()))?;
    let payload = serde_json::from_str::<Value>(&response_text)
        .map_err(|_| AppError::Capture("OpenAI returned an invalid model response.".to_string()))?;
    if !openai_model_lookup_is_usable(&payload, model) {
        return Err(AppError::Capture(
            "OpenAI API key cannot access GPT Live Transcribe.".to_string(),
        ));
    }
    Ok(())
}

/// Converts the normalized `[-1.0, 1.0]` mixed capture stream to the PCM16
/// little-endian audio required by every initial Cloud provider adapter.
pub fn pcm_s16le(samples: &[f32]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| {
            let pcm = if *sample <= -1.0 {
                i16::MIN
            } else if *sample >= 1.0 {
                i16::MAX
            } else {
                (*sample * i16::MAX as f32).round() as i16
            };
            pcm.to_le_bytes()
        })
        .collect()
}

/// Maps documented provider events to a transient partial or persistable final
/// transcript. Unknown control/keepalive events and empty transcripts are not
/// application errors and intentionally produce no event.
pub fn parse_provider_event(
    provider: CloudProvider,
    payload: &str,
) -> Result<Option<CloudTranscriptEvent>> {
    let value: Value = serde_json::from_str(payload)
        .map_err(|_| AppError::Capture("Cloud provider sent an invalid response.".to_string()))?;

    if value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|event_type| event_type.eq_ignore_ascii_case("error"))
    {
        return Err(AppError::Capture(
            "Cloud provider rejected transcription.".to_string(),
        ));
    }

    match provider {
        CloudProvider::Deepgram => parse_deepgram(&value),
        CloudProvider::AssemblyAi => parse_assembly_ai(&value),
        CloudProvider::OpenAi => parse_open_ai(&value),
    }
}

/// Identifies the terminal response for one explicitly committed OpenAI audio
/// buffer. This remains true for an empty transcript so shutdown can finish
/// without waiting for a transcript event that will never arrive.
pub fn provider_event_completes_commit(provider: CloudProvider, payload: &str) -> Result<bool> {
    if provider != CloudProvider::OpenAi {
        return Ok(false);
    }
    let value: Value = serde_json::from_str(payload)
        .map_err(|_| AppError::Capture("Cloud provider sent an invalid response.".to_string()))?;
    Ok(value.get("type").and_then(Value::as_str)
        == Some("conversation.item.input_audio_transcription.completed"))
}

/// Reads the small, provider-specific subset of setup events that determine
/// whether a connection can safely receive live audio. OpenAI error text is
/// never exposed; only a narrowly validated code and field name can be shown.
pub fn parse_provider_ready_event(
    provider: CloudProvider,
    payload: &str,
) -> Result<Option<CloudTransportReadyEvent>> {
    let value: Value = serde_json::from_str(payload)
        .map_err(|_| AppError::Capture("Cloud provider sent an invalid response.".to_string()))?;
    if value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|event_type| event_type.eq_ignore_ascii_case("error"))
    {
        let message = match provider {
            CloudProvider::OpenAi => openai_session_configuration_error(&value),
            CloudProvider::Deepgram | CloudProvider::AssemblyAi => {
                "Cloud provider rejected transcription.".to_string()
            }
        };
        return Err(AppError::Capture(message));
    }
    Ok(
        match (provider, value.get("type").and_then(Value::as_str)) {
            (CloudProvider::OpenAi, Some("session.updated")) => {
                Some(CloudTransportReadyEvent::Ready)
            }
            _ => None,
        },
    )
}

fn openai_session_configuration_error(payload: &Value) -> String {
    let mut details = Vec::new();
    if let Some(code) = payload
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .filter(|code| is_safe_openai_error_identifier(code))
    {
        details.push(format!("code: {code}"));
    }
    if let Some(field) = payload
        .get("error")
        .and_then(|error| error.get("param"))
        .and_then(Value::as_str)
        .filter(|field| is_safe_openai_error_identifier(field))
    {
        details.push(format!("field: {field}"));
    }

    let base = "OpenAI rejected the Realtime transcription session configuration.";
    if details.is_empty() {
        base.to_string()
    } else {
        format!("{base} ({})", details.join("; "))
    }
}

fn is_safe_openai_error_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn parse_deepgram(value: &Value) -> Result<Option<CloudTranscriptEvent>> {
    if value.get("type").and_then(Value::as_str) != Some("Results") {
        return Ok(None);
    }
    let text = value
        .pointer("/channel/alternatives/0/transcript")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let language = value
        .pointer("/channel/alternatives/0/languages/0")
        .and_then(Value::as_str)
        .unwrap_or(UNDETECTED_LANGUAGE);
    Ok(transcript_event(
        text,
        value
            .get("is_final")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        language,
    ))
}

fn parse_assembly_ai(value: &Value) -> Result<Option<CloudTranscriptEvent>> {
    if value.get("type").and_then(Value::as_str) != Some("Turn") {
        return Ok(None);
    }
    Ok(transcript_event(
        value
            .get("transcript")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        value
            .get("end_of_turn")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        UNDETECTED_LANGUAGE,
    ))
}

fn parse_open_ai(value: &Value) -> Result<Option<CloudTranscriptEvent>> {
    let event_type = value.get("type").and_then(Value::as_str);
    let item_id = value
        .get("item_id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    match event_type {
        Some("conversation.item.input_audio_transcription.delta") => {
            let text = value
                .get("delta")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if text.is_empty() {
                return Ok(None);
            }
            Ok(Some(CloudTranscriptEvent::Partial {
                item_id,
                text: text.to_string(),
                incremental: true,
            }))
        }
        Some("conversation.item.input_audio_transcription.completed") => {
            let text = value
                .get("transcript")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            if text.is_empty() {
                return Ok(None);
            }
            Ok(Some(CloudTranscriptEvent::Final {
                item_id,
                text: text.to_string(),
                language: UNDETECTED_LANGUAGE.to_string(),
            }))
        }
        _ => Ok(None),
    }
}

fn transcript_event(
    text: &str,
    final_result: bool,
    language: &str,
) -> Option<CloudTranscriptEvent> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(if final_result {
        CloudTranscriptEvent::Final {
            item_id: None,
            text: text.to_string(),
            language: language.to_string(),
        }
    } else {
        CloudTranscriptEvent::Partial {
            item_id: None,
            text: text.to_string(),
            incremental: false,
        }
    })
}

fn openai_model_lookup_is_usable(payload: &Value, requested_model: &str) -> bool {
    payload.get("id").and_then(Value::as_str) == Some(requested_model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_model_lookup_requires_the_requested_model_identifier() {
        assert!(openai_model_lookup_is_usable(
            &serde_json::json!({ "id": "gpt-live-transcribe" }),
            "gpt-live-transcribe",
        ));
        assert!(!openai_model_lookup_is_usable(
            &serde_json::json!({ "id": "gpt-realtime-2.1" }),
            "gpt-live-transcribe",
        ));
        assert!(!openai_model_lookup_is_usable(
            &serde_json::json!({ "object": "model" }),
            "gpt-live-transcribe",
        ));
    }
}
