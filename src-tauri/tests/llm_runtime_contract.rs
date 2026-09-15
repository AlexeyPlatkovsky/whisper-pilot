use std::cell::Cell;
use std::sync::Arc;

use whisperpilot_lib::error::AppError;
use whisperpilot_lib::llm::{
    generate_mfu_with_inference, run_with_token_preflight, BoundedLlmScheduler, LlmJobKind,
    LlmScheduleError, ModelFingerprint, ScheduledLlmJob, SelectedModelCache,
};

// WP-115: repeated jobs for the exact selected asset reuse immutable model
// weights. The fake payload keeps this contract independent of llama.cpp and
// any downloaded GGUF.
#[test]
fn selected_model_fingerprint_loads_once_and_is_reused_by_sequential_jobs() {
    #[derive(Debug)]
    struct FakeModel;

    let fingerprint = ModelFingerprint::new("qwen-3.5-4b", "sha256:model-v1");
    let load_count = Cell::new(0);
    let mut cache = SelectedModelCache::<FakeModel>::default();

    let first = cache
        .get_or_try_load(&fingerprint, || {
            load_count.set(load_count.get() + 1);
            Ok::<_, AppError>(FakeModel)
        })
        .expect("first job loads selected model");
    let second = cache
        .get_or_try_load(&fingerprint, || {
            load_count.set(load_count.get() + 1);
            Ok::<_, AppError>(FakeModel)
        })
        .expect("second job reuses selected model");

    assert_eq!(load_count.get(), 1);
    assert!(Arc::ptr_eq(&first, &second));
}

// The queue deliberately tests both documented outcomes: interactive
// translation outranks queued background work, and capacity overload is an
// explicit result rather than unbounded growth or a second model runtime.
#[test]
fn bounded_scheduler_prioritizes_translation_and_reports_overload() {
    let mut scheduler = BoundedLlmScheduler::new(2);
    scheduler
        .try_enqueue(ScheduledLlmJob::new(1, LlmJobKind::Mfu))
        .expect("queue MFU");
    scheduler
        .try_enqueue(ScheduledLlmJob::new(2, LlmJobKind::Translation))
        .expect("queue translation");

    let overload = scheduler
        .try_enqueue(ScheduledLlmJob::new(3, LlmJobKind::Prettify))
        .expect_err("bounded queue must report overload");
    assert_eq!(overload, LlmScheduleError::QueueFull { capacity: 2 });

    let first = scheduler.start_next().expect("first scheduled job");
    assert_eq!(first.id(), 2);
    assert_eq!(first.kind(), LlmJobKind::Translation);

    // One scheduler owns one model runtime. Even with another job queued, a
    // second executor cannot start until the current lease is completed.
    assert!(scheduler.start_next().is_none());
    scheduler.finish(first.id()).expect("finish translation");

    let second = scheduler.start_next().expect("second scheduled job");
    assert_eq!(second.id(), 1);
    assert_eq!(second.kind(), LlmJobKind::Mfu);
    scheduler.finish(second.id()).expect("finish MFU");
    assert!(scheduler.start_next().is_none());
}

#[test]
fn selected_model_mutation_is_the_next_job_after_active_inference() {
    let mut scheduler = BoundedLlmScheduler::new(4);
    scheduler
        .try_enqueue(ScheduledLlmJob::new(1, LlmJobKind::Mfu))
        .expect("queue active MFU");
    let active = scheduler.start_next().expect("start MFU");

    scheduler
        .try_enqueue(ScheduledLlmJob::new(2, LlmJobKind::Prettify))
        .expect("queue background prettify");
    scheduler
        .try_enqueue(ScheduledLlmJob::new(3, LlmJobKind::Translation))
        .expect("queue translation");
    scheduler
        .try_enqueue(ScheduledLlmJob::new(4, LlmJobKind::ModelMutation))
        .expect("queue selected-model mutation barrier");

    scheduler.finish(active.id()).expect("finish active MFU");
    let mutation = scheduler.start_next().expect("start mutation barrier");
    assert_eq!(mutation.kind(), LlmJobKind::ModelMutation);
    scheduler
        .finish(mutation.id())
        .expect("finish mutation barrier");

    let translation = scheduler.start_next().expect("start translation");
    assert_eq!(translation.kind(), LlmJobKind::Translation);
}

#[test]
fn committed_translation_overtakes_a_queued_preview() {
    let mut scheduler = BoundedLlmScheduler::new(4);
    scheduler
        .try_enqueue(ScheduledLlmJob::new(1, LlmJobKind::Mfu))
        .expect("queue active MFU");
    let active = scheduler.start_next().expect("start MFU");

    scheduler
        .try_enqueue(ScheduledLlmJob::new(2, LlmJobKind::TranslationPreview))
        .expect("queue provisional translation");
    scheduler
        .try_enqueue(ScheduledLlmJob::new(3, LlmJobKind::Translation))
        .expect("queue committed translation");

    scheduler.finish(active.id()).expect("finish MFU");
    let committed = scheduler.start_next().expect("start committed translation");
    assert_eq!(committed.id(), 3);
    assert_eq!(committed.kind(), LlmJobKind::Translation);
    scheduler
        .finish(committed.id())
        .expect("finish committed translation");

    let preview = scheduler.start_next().expect("start preview");
    assert_eq!(preview.id(), 2);
    assert_eq!(preview.kind(), LlmJobKind::TranslationPreview);
}

// Token counting is injected so this proves the ordering contract without a
// real model: an overflowing prompt is rejected before the decode callback.
#[test]
fn token_preflight_rejects_prompt_plus_reserved_output_before_decode() {
    const CONTEXT_SIZE: usize = 16_384;
    const RESERVED_OUTPUT: usize = 1_024;
    let decode_calls = Cell::new(0);
    let prompt_tokens = CONTEXT_SIZE - RESERVED_OUTPUT + 1;

    let error = run_with_token_preflight(prompt_tokens, RESERVED_OUTPUT, CONTEXT_SIZE, || {
        decode_calls.set(decode_calls.get() + 1);
        Ok::<_, AppError>("must not decode".to_string())
    })
    .expect_err("prompt plus reserved output exceeds context");

    assert!(matches!(error, AppError::Llm(_)));
    assert!(error.to_string().contains("context"));
    assert_eq!(decode_calls.get(), 0);
}

#[test]
fn token_preflight_accepts_the_exact_context_boundary() {
    const CONTEXT_SIZE: usize = 16_384;
    const RESERVED_OUTPUT: usize = 1_024;
    let decode_calls = Cell::new(0);

    run_with_token_preflight(
        CONTEXT_SIZE - RESERVED_OUTPUT,
        RESERVED_OUTPUT,
        CONTEXT_SIZE,
        || {
            decode_calls.set(decode_calls.get() + 1);
            Ok::<_, AppError>(())
        },
    )
    .expect("the last valid prompt/output boundary must fit");

    assert_eq!(decode_calls.get(), 1);
}

// The injected inference seam proves malformed model output is a per-job
// failure without loading llama.cpp or accepting a summary-only fallback.
#[test]
fn malformed_mfu_json_is_an_explicit_error() {
    let error = generate_mfu_with_inference("Alex: We approved the launch.", |_prompt| {
        Ok::<_, AppError>("This is not JSON.".to_string())
    })
    .expect_err("malformed MFU JSON must fail the job");

    assert!(matches!(error, AppError::Llm(_)));
    assert!(error.to_string().contains("valid structured MFU"));
}

#[test]
#[ignore = "loads a local GGUF and measures real inference latency"]
fn repeated_real_jobs_measure_the_cached_model_speedup() {
    let Some(path) = std::env::var_os("WHISPERPILOT_TEST_LLM_MODEL").map(std::path::PathBuf::from)
    else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_LLM_MODEL to a local GGUF");
        return;
    };
    let transcript = "Alex: We approved the release. Sam: I will publish it on Friday.";
    let runtime = whisperpilot_lib::llm::LlmRuntime::default();

    let started = std::time::Instant::now();
    let first = whisperpilot_lib::llm::generate_mfu(&runtime, &path, transcript)
        .expect("first real MFU generation succeeds");
    let first_elapsed = started.elapsed();

    let started = std::time::Instant::now();
    let second = whisperpilot_lib::llm::generate_mfu(&runtime, &path, transcript)
        .expect("second real MFU generation succeeds");
    let second_elapsed = started.elapsed();

    assert!(!first.summary.is_empty() && !second.summary.is_empty());
    eprintln!("real LLM latency: first={first_elapsed:?}, cached={second_elapsed:?}");
    assert!(
        second_elapsed < first_elapsed,
        "the cached job should avoid first-run model load latency"
    );
}

#[test]
#[ignore = "loads a pinned GGUF and runs the frozen multilingual product corpus"]
fn pinned_model_passes_multilingual_translation_polish_and_mfu_smoke() {
    let Some(path) = std::env::var_os("WHISPERPILOT_TEST_LLM_MODEL").map(std::path::PathBuf::from)
    else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_LLM_MODEL to a local GGUF");
        return;
    };
    let runtime = whisperpilot_lib::llm::LlmRuntime::default();
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/llm_profile_corpus.json"))
            .expect("frozen LLM profile corpus is valid JSON");
    assert_eq!(corpus["revision"], 1);

    let started = std::time::Instant::now();
    let into_english = whisperpilot_lib::llm::translate_paragraph(
        &runtime,
        &path,
        "Алексей отправит build WP-129 в 15:30, бюджет 42 USD.",
        "en",
        None,
    )
    .expect("Russian to English translation");
    for protected in ["WP-129", "15:30", "42"] {
        assert!(
            into_english.contains(protected),
            "missing {protected}: {into_english}"
        );
    }

    let scientific = whisperpilot_lib::llm::translate_paragraph(
        &runtime,
        &path,
        "Проблема существования и гладкости уравнений Навье-Стокса оставалась открытой.",
        "en",
        None,
    )
    .expect("cross-script hyphenated scientific name translates");
    let scientific_lower = scientific.to_lowercase();
    assert!(
        scientific_lower.contains("navier") && scientific_lower.contains("stokes"),
        "unexpected scientific translation: {scientific}"
    );

    let into_russian = whisperpilot_lib::llm::translate_paragraph(
        &runtime,
        &path,
        "Sam will publish build WP-129 at 15:30 with budget 42 USD.",
        "ru",
        Some("The release was approved."),
    )
    .expect("English to Russian translation");
    for protected in ["WP-129", "15:30", "42"] {
        assert!(
            into_russian.contains(protected),
            "missing {protected}: {into_russian}"
        );
    }

    let polished = whisperpilot_lib::llm::prettify_transcript(
        &runtime,
        &path,
        "Ну, ну, Alex, отправь build WP-129 в 15:30, budget 42 USD.",
    )
    .expect("mixed-language Recorder polish");
    for protected in ["Alex", "WP-129", "15:30", "42", "USD"] {
        assert!(
            polished.contains(protected),
            "missing {protected}: {polished}"
        );
    }

    let mfu = whisperpilot_lib::llm::generate_mfu(
        &runtime,
        &path,
        "Алексей: Релиз WP-129 approved. Sam: Я отправлю build 42 в 15:30.",
    )
    .expect("mixed-language MFU schema");
    assert!(!mfu.summary.is_empty());
    assert!(!mfu.action_items.is_empty());
    let long_transcript = "Алексей: Релиз WP-129 approved. Sam: Я отправлю build 42 в 15:30. Morgan: Keep API_v2 unchanged and budget at 42 USD.\n".repeat(40);
    let long_mfu = whisperpilot_lib::llm::generate_mfu(&runtime, &path, &long_transcript)
        .expect("representative long mixed-language MFU schema");
    assert!(!long_mfu.summary.is_empty());
    assert!(!long_mfu.action_items.is_empty());
    eprintln!(
        "multilingual corpus elapsed={:?}; en={into_english:?}; ru={into_russian:?}; polish={polished:?}",
        started.elapsed()
    );
}
