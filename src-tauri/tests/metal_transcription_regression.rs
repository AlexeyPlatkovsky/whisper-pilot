//! Real-Metal regression coverage for Meeting transcription (WP-86).
//!
//! This test is ignored by the default suite because it loads the production
//! Whisper model and runs a native GPU decode. Run it on macOS with:
//!   cargo test --manifest-path src-tauri/Cargo.toml --test metal_transcription_regression -- --ignored --nocapture

#![cfg(target_os = "macos")]

use std::ffi::{c_char, c_void, CStr};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use whisper_rs::{WhisperContext, WhisperContextParameters};

static WHISPER_LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static PROGRESS_CALLBACK_DROPPED: AtomicBool = AtomicBool::new(false);
static PROGRESS_AFTER_DROP: AtomicBool = AtomicBool::new(false);
static PROGRESS_CALLS: AtomicUsize = AtomicUsize::new(0);

struct ProgressLifetimeProbe;

impl Drop for ProgressLifetimeProbe {
    fn drop(&mut self) {
        PROGRESS_CALLBACK_DROPPED.store(true, Ordering::SeqCst);
    }
}

unsafe extern "C" fn capture_whisper_log(
    _level: std::os::raw::c_uint,
    text: *const c_char,
    _user_data: *mut c_void,
) {
    if text.is_null() {
        return;
    }
    let message = unsafe { CStr::from_ptr(text) }.to_string_lossy();
    if let Ok(mut logs) = WHISPER_LOGS.lock() {
        logs.push(message.into_owned());
    }
}

fn production_model_path() -> PathBuf {
    if let Some(path) = std::env::var_os("WHISPERPILOT_TEST_MODEL") {
        return PathBuf::from(path);
    }
    dirs_next::data_local_dir()
        .or_else(|| dirs_next::home_dir().map(|home| home.join("Library/Application Support")))
        .expect("macOS application-support directory")
        .join("com.whisperpilot.dev/models/ggml-large-v3-turbo-q8_0.bin")
}

fn generated_speech() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temporary speech-fixture directory");
    let path = dir.path().join("metal-regression.aiff");
    let speech = "Whisper Pilot is testing offline transcription on the Metal graphics processor. \
        This sentence is repeated to provide a stable spoken recording for the encoder. \
        Whisper Pilot is testing offline transcription on the Metal graphics processor. \
        This sentence is repeated to provide a stable spoken recording for the encoder. \
        Whisper Pilot is testing offline transcription on the Metal graphics processor. \
        This sentence is repeated to provide a stable spoken recording for the encoder.";
    let status = Command::new("/usr/bin/say")
        .args(["-v", "Samantha", "-r", "175", "-o"])
        .arg(&path)
        .arg(speech)
        .status()
        .expect("launch the macOS speech synthesizer");
    assert!(status.success(), "macOS speech fixture generation failed");
    (dir, path)
}

/// C-1/C-2: Meeting must return speech on Metal, and Streaming's native
/// progress callback must remain alive throughout a real synchronous decode.
#[test]
#[ignore = "loads the production Whisper model and performs a real Metal decode"]
fn meeting_transcription_uses_metal_and_produces_speech_segments() {
    WHISPER_LOGS.lock().expect("Whisper log lock").clear();
    unsafe {
        whisper_rs::set_log_callback(Some(capture_whisper_log), std::ptr::null_mut());
    }

    let model = production_model_path();
    assert!(
        model.is_file(),
        "production model is required at {} (or set WHISPERPILOT_TEST_MODEL)",
        model.display()
    );
    let mut params = WhisperContextParameters::default();
    params.use_gpu(true);
    params.flash_attn(true);
    let ctx = WhisperContext::new_with_params(model.to_str().expect("UTF-8 model path"), params)
        .expect("load the production Whisper model");

    let backend_logs = WHISPER_LOGS.lock().expect("Whisper log lock").join("");
    assert!(
        backend_logs.contains("use gpu    = 1")
            && backend_logs.contains("ggml_metal_device_init: GPU name:")
            && (backend_logs.contains("Metal total size")
                || backend_logs.contains("MTL0 total size")),
        "the regression test must initialize Metal and load the model onto the GPU; native log was:\n{backend_logs}"
    );

    let (_fixture, audio) = generated_speech();
    let samples = whisperpilot_lib::audio::load_samples(&audio).expect("decode speech fixture");
    assert!(
        samples.len() > whisperpilot_lib::audio::SAMPLE_RATE as usize * 10,
        "speech fixture must be long enough to exercise the encoder"
    );
    assert!(
        samples.iter().all(|sample| sample.is_finite()),
        "speech fixture contains non-finite samples"
    );
    let peak = samples
        .iter()
        .copied()
        .map(f32::abs)
        .fold(0.0_f32, f32::max);
    assert!(
        peak > 0.01,
        "speech fixture is silent (peak amplitude {peak})"
    );

    let transcription = whisperpilot_lib::transcribe::transcribe(&ctx, &samples)
        .expect("Metal transcription must complete");
    assert!(
        !transcription.segments.is_empty(),
        "Metal transcription returned no speech segments; native log was:\n{}",
        WHISPER_LOGS.lock().expect("Whisper log lock").join("")
    );

    PROGRESS_CALLBACK_DROPPED.store(false, Ordering::SeqCst);
    PROGRESS_AFTER_DROP.store(false, Ordering::SeqCst);
    PROGRESS_CALLS.store(0, Ordering::SeqCst);
    let mut streaming_state = ctx.create_state().expect("create Streaming decoder state");
    let probe = ProgressLifetimeProbe;
    whisperpilot_lib::transcribe::transcribe_with_state(
        &mut streaming_state,
        &samples,
        move |_percent| {
            std::hint::black_box(&probe);
            PROGRESS_CALLS.fetch_add(1, Ordering::SeqCst);
            if PROGRESS_CALLBACK_DROPPED.load(Ordering::SeqCst) {
                PROGRESS_AFTER_DROP.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("Streaming-style Metal transcription must complete");
    assert!(
        PROGRESS_CALLS.load(Ordering::SeqCst) > 0,
        "the real Whisper decode did not invoke its progress callback"
    );
    assert!(
        !PROGRESS_AFTER_DROP.load(Ordering::SeqCst),
        "Whisper invoked the progress callback after its captured state was dropped"
    );
}
