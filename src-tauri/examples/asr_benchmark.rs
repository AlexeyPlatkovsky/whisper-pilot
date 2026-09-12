//! Phase 4 local benchmark harness. This is intentionally outside production
//! paths; see docs/validation/phase-4-qwen-asr-qualification.md.

#[cfg(target_os = "macos")]
fn main() {
    use qwen_asr::context::QwenCtx;
    use qwen_asr::transcribe::{self, StreamState};
    use std::time::Instant;

    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() == 5 || args.len() == 6,
        "usage: asr_benchmark ENGINE MODEL_PATH WAV LANGUAGE [REPEATS]"
    );
    let samples = load_wav(&args[3]);
    let audio_seconds = samples.len() as f64 / 16_000.0;
    match args[1].as_str() {
        "qwen" => {
            let load_started = Instant::now();
            let mut context = QwenCtx::load(&args[2]).expect("load Qwen3-ASR bundle");
            let load_ms = load_started.elapsed().as_secs_f64() * 1_000.0;
            configure_qwen(&mut context, &args[4]);
            context.segment_sec = 30.0;
            if let Some(repeats) = args.get(5) {
                let repeats: usize = repeats.parse().expect("numeric repeats");
                let started = Instant::now();
                for index in 0..repeats {
                    let text = transcribe::transcribe_audio(&mut context, &samples)
                        .expect("Qwen stability decode");
                    assert!(!text.trim().is_empty(), "empty output at iteration {index}");
                }
                println!(
                    "engine=qwen repeats={repeats} audio_minutes={:.2} wall_ms={:.1} peak_rss=measure-with-/usr/bin/time",
                    audio_seconds * repeats as f64 / 60.0,
                    started.elapsed().as_secs_f64() * 1_000.0,
                );
                return;
            }
            let started = Instant::now();
            let text =
                transcribe::transcribe_audio(&mut context, &samples).expect("Qwen offline decode");
            let final_ms = started.elapsed().as_secs_f64() * 1_000.0;

            let mut streaming_context = context.model().new_session();
            configure_qwen(&mut streaming_context, &args[4]);
            let mut stream = StreamState::new();
            let stream_started = Instant::now();
            let first_end = samples.len().min(8_000);
            let probe = transcribe::stream_push_audio(
                &mut streaming_context,
                &samples[..first_end],
                &mut stream,
                false,
            )
            .expect("Qwen streaming probe");
            let probe_ms = stream_started.elapsed().as_secs_f64() * 1_000.0;
            println!(
                "engine=qwen load_ms={load_ms:.1} final_ms={final_ms:.1} half_second_probe_ms={probe_ms:.1} probe_nonempty={} rtf={:.4}",
                !probe.trim().is_empty(),
                final_ms / 1_000.0 / audio_seconds
            );
            println!("text={text}");
        }
        "whisper" => {
            use whisper_rs::{WhisperContext, WhisperContextParameters};
            let load_started = Instant::now();
            let mut params = WhisperContextParameters::default();
            params.flash_attn(true);
            let context =
                WhisperContext::new_with_params(&args[2], params).expect("load Whisper model");
            let load_ms = load_started.elapsed().as_secs_f64() * 1_000.0;
            let started = Instant::now();
            let result = whisperpilot_lib::transcribe::transcribe(&context, &samples)
                .expect("Whisper decode");
            let final_ms = started.elapsed().as_secs_f64() * 1_000.0;
            let text = result
                .segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            println!(
                "engine=whisper load_ms={load_ms:.1} final_ms={final_ms:.1} rtf={:.4} language={}",
                final_ms / 1_000.0 / audio_seconds,
                result.language
            );
            println!("text={text}");
        }
        other => panic!("unknown engine {other}; expected qwen or whisper"),
    }
}

#[cfg(target_os = "macos")]
fn configure_qwen(context: &mut qwen_asr::context::QwenCtx, language: &str) {
    match language {
        "ru" => context.set_force_language("Russian").unwrap(),
        "en" => context.set_force_language("English").unwrap(),
        "mixed" => context.set_multilingual(true),
        other => panic!("unknown language {other}; expected ru, en, or mixed"),
    }
}

#[cfg(target_os = "macos")]
fn load_wav(path: &str) -> Vec<f32> {
    let mut reader = hound::WavReader::open(path).expect("open WAV");
    let spec = reader.spec();
    assert_eq!(spec.channels, 1, "benchmark WAV must be mono");
    assert_eq!(spec.sample_rate, 16_000, "benchmark WAV must be 16 kHz");
    reader
        .samples::<i16>()
        .map(|sample| sample.expect("PCM16 sample") as f32 / 32_768.0)
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Phase 4 ASR benchmark requires Apple Silicon macOS");
    std::process::exit(2);
}
