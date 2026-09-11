use whisperpilot_lib::error::AppError;
use whisperpilot_lib::streaming_audio::system_audio_capture_spec;

#[test]
fn streaming_capture_is_system_audio_only_at_local_and_openai_rates() {
    let local = system_audio_capture_spec(16_000).unwrap();
    assert_eq!(local.sample_rate, 16_000);
    assert!(!local.microphone);
    assert!(local.system_audio);

    let openai = system_audio_capture_spec(24_000).unwrap();
    assert_eq!(openai.sample_rate, 24_000);
    assert!(!openai.microphone);
    assert!(openai.system_audio);
}

#[test]
fn streaming_capture_rejects_a_rate_screen_capture_kit_does_not_support() {
    let error = system_audio_capture_spec(44_100).unwrap_err();
    assert!(matches!(error, AppError::Capture(_)));
}
