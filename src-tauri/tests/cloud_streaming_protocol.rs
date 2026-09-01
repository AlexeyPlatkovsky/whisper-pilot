use whisperpilot_lib::cloud_provider::CloudProvider;
use whisperpilot_lib::cloud_streaming::{
    connection_spec, outbound_audio_message, outbound_commit_message, parse_provider_event,
    parse_provider_ready_event, pcm_s16le, provider_event_completes_commit, CloudOutboundMessage,
    CloudTranscriptAssembler, CloudTranscriptEvent, CloudTransportReadyEvent,
};

#[test]
fn encodes_normalized_capture_samples_as_clamped_pcm16_little_endian() {
    let bytes = pcm_s16le(&[-1.5, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5]);
    let samples = bytes
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
        .collect::<Vec<_>>();

    assert_eq!(
        samples,
        vec![i16::MIN, i16::MIN, -16_384, 0, 16_384, i16::MAX, i16::MAX]
    );
}

#[test]
fn maps_deepgram_interim_and_final_results_without_persisting_empty_text() {
    let interim = parse_provider_event(
        CloudProvider::Deepgram,
        r#"{"type":"Results","is_final":false,"channel":{"alternatives":[{"transcript":"hello"}]}}"#,
    )
    .unwrap();
    assert_eq!(
        interim,
        Some(CloudTranscriptEvent::Partial {
            item_id: None,
            text: "hello".into(),
            incremental: false,
        })
    );

    let final_result = parse_provider_event(
        CloudProvider::Deepgram,
        r#"{"type":"Results","is_final":true,"channel":{"alternatives":[{"transcript":"hello world","languages":["en-US"]}]}}"#,
    )
    .unwrap();
    assert_eq!(
        final_result,
        Some(CloudTranscriptEvent::Final {
            item_id: None,
            text: "hello world".into(),
            language: "en-US".into(),
        })
    );

    assert_eq!(
        parse_provider_event(
            CloudProvider::Deepgram,
            r#"{"type":"Results","is_final":true,"channel":{"alternatives":[{"transcript":"   "}]}}"#,
        )
        .unwrap(),
        None
    );
}

#[test]
fn maps_assemblyai_turns_and_openai_realtime_transcription_events() {
    assert_eq!(
        parse_provider_event(
            CloudProvider::AssemblyAi,
            r#"{"type":"Turn","transcript":"partial","end_of_turn":false}"#,
        )
        .unwrap(),
        Some(CloudTranscriptEvent::Partial {
            item_id: None,
            text: "partial".into(),
            incremental: false,
        })
    );
    assert_eq!(
        parse_provider_event(
            CloudProvider::AssemblyAi,
            r#"{"type":"Turn","transcript":"final turn","end_of_turn":true}"#,
        )
        .unwrap(),
        Some(CloudTranscriptEvent::Final {
            item_id: None,
            text: "final turn".into(),
            language: "auto".into(),
        })
    );
    assert_eq!(
        parse_provider_event(
            CloudProvider::OpenAi,
            r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"turn-a","delta":"partial"}"#,
        )
        .unwrap(),
        Some(CloudTranscriptEvent::Partial {
            item_id: Some("turn-a".into()),
            text: "partial".into(),
            incremental: true,
        })
    );
    assert_eq!(
        parse_provider_event(
            CloudProvider::OpenAi,
            r#"{"type":"conversation.item.input_audio_transcription.completed","item_id":"turn-a","transcript":"final turn"}"#,
        )
        .unwrap(),
        Some(CloudTranscriptEvent::Final {
            item_id: Some("turn-a".into()),
            text: "final turn".into(),
            language: "auto".into(),
        })
    );
}

#[test]
fn accumulates_openai_deltas_per_item_without_flashing_or_joining_words() {
    let mut assembler = CloudTranscriptAssembler::default();

    let first = parse_provider_event(
        CloudProvider::OpenAi,
        r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"turn-a","delta":"Hello"}"#,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        assembler.apply(first),
        CloudTranscriptEvent::Partial {
            item_id: Some("turn-a".into()),
            text: "Hello".into(),
            incremental: false,
        }
    );

    let second = parse_provider_event(
        CloudProvider::OpenAi,
        r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"turn-a","delta":" world"}"#,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        assembler.apply(second),
        CloudTranscriptEvent::Partial {
            item_id: Some("turn-a".into()),
            text: "Hello world".into(),
            incremental: false,
        }
    );

    let other_turn = parse_provider_event(
        CloudProvider::OpenAi,
        r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"turn-b","delta":"Separate"}"#,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        assembler.apply(other_turn),
        CloudTranscriptEvent::Partial {
            item_id: Some("turn-b".into()),
            text: "Separate".into(),
            incremental: false,
        }
    );

    let completed = parse_provider_event(
        CloudProvider::OpenAi,
        r#"{"type":"conversation.item.input_audio_transcription.completed","item_id":"turn-a","transcript":"Hello world"}"#,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        assembler.apply(completed),
        CloudTranscriptEvent::Final {
            item_id: Some("turn-a".into()),
            text: "Hello world".into(),
            language: "auto".into(),
        }
    );
}

#[test]
fn rejects_malformed_provider_payloads_without_echoing_them() {
    let error = parse_provider_event(CloudProvider::OpenAi, "not json").unwrap_err();
    assert!(error
        .to_string()
        .contains("Cloud provider sent an invalid response"));
    assert!(!error.to_string().contains("not json"));
}

#[test]
fn rejects_provider_error_events_without_exposing_remote_detail() {
    let error = parse_provider_event(
        CloudProvider::OpenAi,
        r#"{"type":"error","error":{"message":"invalid api key: test-secret"}}"#,
    )
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("Cloud provider rejected transcription"));
    assert!(!error.to_string().contains("test-secret"));
}

// Session startup must wait for OpenAI to accept the requested transcription
// session, instead of treating a successful TCP/WebSocket upgrade as
// authorization. Key verification deliberately uses a separate no-session
// model-access lookup.
#[test]
fn openai_readiness_requires_session_update_acceptance_and_redacts_rejections() {
    assert_eq!(
        parse_provider_ready_event(
            CloudProvider::OpenAi,
            r#"{"type":"session.created","session":{"id":"sess_123"}}"#,
        )
        .unwrap(),
        None
    );
    assert_eq!(
        parse_provider_ready_event(
            CloudProvider::OpenAi,
            r#"{"type":"session.updated","session":{"type":"transcription"}}"#,
        )
        .unwrap(),
        Some(CloudTransportReadyEvent::Ready)
    );

    let error = parse_provider_ready_event(
        CloudProvider::OpenAi,
        r#"{"type":"error","error":{"code":"invalid_value","param":"session.audio.input.transcription.model","message":"invalid api key: test-secret"}}"#,
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("OpenAI rejected the Realtime transcription session configuration"));
    assert!(message.contains("code: invalid_value"));
    assert!(message.contains("field: session.audio.input.transcription.model"));
    assert!(!message.contains("test-secret"));

    // EP: malformed remote identifiers are an unsafe diagnostic class and
    // must not be echoed, even when a provider sends them alongside a secret.
    let unsafe_error = parse_provider_ready_event(
        CloudProvider::OpenAi,
        r#"{"type":"error","error":{"code":"invalid value: test-secret","param":"session\naudio","message":"test-secret"}}"#,
    )
    .unwrap_err();
    let unsafe_message = unsafe_error.to_string();
    assert!(!unsafe_message.contains("test-secret"));
    assert!(!unsafe_message.contains("invalid value"));
    assert!(!unsafe_message.contains("session\naudio"));
}

#[test]
fn builds_the_documented_provider_connections_without_putting_the_key_in_the_url() {
    let deepgram = connection_spec(CloudProvider::Deepgram, "test-key");
    assert_eq!(deepgram.url, "wss://api.deepgram.com/v1/listen?model=nova-3&encoding=linear16&sample_rate=16000&channels=1&interim_results=true&smart_format=true&endpointing=300");
    assert_eq!(deepgram.authorization, "Token test-key");
    assert_eq!(deepgram.sample_rate, 16_000);
    assert!(deepgram.initial_messages.is_empty());
    assert!(!deepgram.url.contains("test-key"));

    let assembly_ai = connection_spec(CloudProvider::AssemblyAi, "test-key");
    assert_eq!(
        assembly_ai.url,
        "wss://streaming.assemblyai.com/v3/ws?sample_rate=16000&speech_model=universal-3-5-pro"
    );
    assert_eq!(assembly_ai.authorization, "test-key");
    assert_eq!(assembly_ai.sample_rate, 16_000);
    assert!(assembly_ai.initial_messages.is_empty());
    assert!(!assembly_ai.url.contains("test-key"));

    let open_ai = connection_spec(CloudProvider::OpenAi, "test-key");
    assert_eq!(
        open_ai.url,
        "wss://api.openai.com/v1/realtime?intent=transcription"
    );
    assert!(!open_ai.url.contains("model="));
    assert_eq!(open_ai.authorization, "Bearer test-key");
    assert_eq!(open_ai.sample_rate, 24_000);
    assert_eq!(open_ai.initial_messages.len(), 1);
    let update: serde_json::Value = serde_json::from_str(&open_ai.initial_messages[0]).unwrap();
    assert_eq!(update["type"], "session.update");
    assert_eq!(update["session"]["type"], "transcription");
    assert_eq!(
        update["session"]["audio"]["input"]["format"]["rate"],
        24_000
    );
    assert_eq!(
        update["session"]["audio"]["input"]["transcription"]["model"],
        "gpt-live-transcribe"
    );
    assert_eq!(
        update["session"]["audio"]["input"]["transcription"]["delay"],
        "high"
    );
    assert_eq!(
        update["session"]["audio"]["input"]["transcription"]["prompt"],
        "A professional meeting with natural pauses that may include names, numbers, acronyms, and technical terms."
    );
    let turn_detection = update["session"]["audio"]["input"]
        .get("turn_detection")
        .expect("GPT Live Transcribe requires turn detection to be explicitly disabled");
    assert!(turn_detection.is_null());
    assert!(!open_ai.url.contains("test-key"));
}

#[test]
fn encodes_audio_as_binary_for_raw_pcm_providers_and_base64_realtime_events_for_openai() {
    let samples = [0.0, 0.5];
    let deepgram = connection_spec(CloudProvider::Deepgram, "test-key");
    let CloudOutboundMessage::Binary(audio) = outbound_audio_message(&deepgram, &samples) else {
        panic!("Deepgram must receive raw PCM audio");
    };
    assert_eq!(audio, pcm_s16le(&samples));

    let open_ai = connection_spec(CloudProvider::OpenAi, "test-key");
    let CloudOutboundMessage::Text(payload) = outbound_audio_message(&open_ai, &samples) else {
        panic!("OpenAI must receive a text realtime event");
    };
    let event: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(event["type"], "input_audio_buffer.append");
    assert!(event["audio"]
        .as_str()
        .is_some_and(|audio| !audio.is_empty()));
}

#[test]
fn commits_openai_audio_periodically_and_flushes_only_non_empty_final_audio() {
    assert!(outbound_commit_message(CloudProvider::OpenAi, 0, true).is_none());
    assert!(outbound_commit_message(CloudProvider::OpenAi, 111_999, false).is_none());

    let Some(CloudOutboundMessage::Text(periodic_payload)) =
        outbound_commit_message(CloudProvider::OpenAi, 112_000, false)
    else {
        panic!("seven seconds of OpenAI audio must create a commit event");
    };
    let periodic_event: serde_json::Value = serde_json::from_str(&periodic_payload).unwrap();
    assert_eq!(periodic_event["type"], "input_audio_buffer.commit");

    let Some(CloudOutboundMessage::Text(final_payload)) =
        outbound_commit_message(CloudProvider::OpenAi, 1, true)
    else {
        panic!("remaining OpenAI audio must be committed before shutdown");
    };
    let final_event: serde_json::Value = serde_json::from_str(&final_payload).unwrap();
    assert_eq!(final_event["type"], "input_audio_buffer.commit");

    assert!(outbound_commit_message(CloudProvider::Deepgram, 112_000, true).is_none());
    assert!(outbound_commit_message(CloudProvider::AssemblyAi, 112_000, true).is_none());
}

#[test]
fn recognizes_openai_commit_completion_even_when_the_transcript_is_empty() {
    let completed =
        r#"{"type":"conversation.item.input_audio_transcription.completed","transcript":""}"#;
    assert!(provider_event_completes_commit(CloudProvider::OpenAi, completed).unwrap());
    assert!(!provider_event_completes_commit(
        CloudProvider::OpenAi,
        r#"{"type":"conversation.item.input_audio_transcription.delta","delta":"partial"}"#,
    )
    .unwrap());
    assert!(!provider_event_completes_commit(CloudProvider::Deepgram, completed).unwrap());
}
