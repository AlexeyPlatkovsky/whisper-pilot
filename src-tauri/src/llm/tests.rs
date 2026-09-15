use super::*;
use super::{policy::*, runtime::*, translation::*};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn wait_until_pending(runtime: &LlmRuntime, kind: LlmJobKind) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let queued = runtime
            .schedule
            .lock()
            .expect("scheduler lock")
            .pending
            .iter()
            .any(|job| job.kind() == kind);
        if queued {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {kind:?} to enter the scheduler"
        );
        std::thread::yield_now();
    }
}

fn loaded_test_cache() -> Arc<Mutex<SelectedModelCache<String>>> {
    let mut cache = SelectedModelCache::default();
    cache
        .get_or_try_load(&ModelFingerprint::new("test", "v1"), || {
            Ok("loaded model".to_string())
        })
        .expect("load fake model");
    Arc::new(Mutex::new(cache))
}

fn wait_until_cache_is_empty(cache: &Arc<Mutex<SelectedModelCache<String>>>) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while cache.lock().expect("cache lock").selected.is_some() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for idle model unload"
        );
        std::thread::yield_now();
    }
}

#[test]
fn text_llm_cache_unloads_after_two_minutes_of_idle_time() {
    assert_eq!(LLM_MODEL_IDLE_TIMEOUT, Duration::from_secs(120));
    let schedule = Arc::new(Mutex::new(BoundedLlmScheduler::new(1)));
    let cache = loaded_test_cache();
    let unloader = IdleModelUnloader::new(
        Arc::clone(&schedule),
        Arc::clone(&cache),
        Duration::from_millis(20),
    );

    unloader.reset();
    wait_until_cache_is_empty(&cache);
}

#[test]
fn text_llm_idle_unload_waits_for_active_work_and_resets_after_completion() {
    let schedule = Arc::new(Mutex::new(BoundedLlmScheduler::new(1)));
    let cache = loaded_test_cache();
    let unloader = IdleModelUnloader::new(
        Arc::clone(&schedule),
        Arc::clone(&cache),
        Duration::from_millis(30),
    );
    let job = ScheduledLlmJob::new(1, LlmJobKind::Prettify);
    {
        let mut guard = schedule.lock().expect("scheduler lock");
        guard.try_enqueue(job).expect("queue active job");
        assert_eq!(guard.start_next(), Some(job));
    }

    unloader.reset();
    std::thread::sleep(Duration::from_millis(60));
    assert!(cache.lock().expect("cache lock").selected.is_some());

    schedule
        .lock()
        .expect("scheduler lock")
        .finish(job.id())
        .expect("finish active job");
    unloader.reset();
    wait_until_cache_is_empty(&cache);
}

#[test]
fn text_llm_idle_eviction_blocks_a_new_lease_until_cache_clear_finishes() {
    let schedule = Arc::new(Mutex::new(BoundedLlmScheduler::new(1)));
    let cache = loaded_test_cache();
    let cache_guard = cache.lock().expect("hold cache during idle expiry");
    let unloader = IdleModelUnloader::new(
        Arc::clone(&schedule),
        Arc::clone(&cache),
        Duration::from_millis(20),
    );

    unloader.reset();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match schedule.try_lock() {
            Err(std::sync::TryLockError::WouldBlock) => break,
            Err(std::sync::TryLockError::Poisoned(_)) => {
                panic!("scheduler lock poisoned")
            }
            Ok(guard) => drop(guard),
        }
        assert!(
            Instant::now() < deadline,
            "idle eviction never held the scheduler while awaiting cache clear"
        );
        std::thread::yield_now();
    }

    drop(cache_guard);
    let _schedule_guard = schedule.lock().expect("new lease after eviction");
    assert!(cache.lock().expect("cache lock").selected.is_none());
}

#[test]
fn queued_old_generation_does_not_resolve_model_after_mutation_barrier() {
    let runtime = Arc::new(LlmRuntime::default());
    let blocker_id = u64::MAX;
    {
        let mut schedule = runtime.schedule.lock().expect("scheduler lock");
        schedule
            .try_enqueue(ScheduledLlmJob::new(blocker_id, LlmJobKind::Mfu))
            .expect("queue blocker");
        assert_eq!(
            schedule.start_next().map(ScheduledLlmJob::id),
            Some(blocker_id)
        );
    }

    let resolver_called = Arc::new(AtomicBool::new(false));
    let inference_runtime = Arc::clone(&runtime);
    let inference_resolver_called = Arc::clone(&resolver_called);
    let inference = std::thread::spawn(move || {
        inference_runtime.infer_with_model_resolver(LlmJobKind::Prettify, "hello", || {
            inference_resolver_called.store(true, Ordering::Release);
            Err(AppError::Llm("resolver must not run".to_string()))
        })
    });
    wait_until_pending(&runtime, LlmJobKind::Prettify);

    let mutation_runtime = Arc::clone(&runtime);
    let mutation = std::thread::spawn(move || {
        mutation_runtime.mutate_selected_model(|| Ok::<_, AppError>(()))
    });
    wait_until_pending(&runtime, LlmJobKind::ModelMutation);

    {
        let mut schedule = runtime.schedule.lock().expect("scheduler lock");
        schedule.finish(blocker_id).expect("finish blocker");
        assert_eq!(
            schedule.start_next().map(ScheduledLlmJob::kind),
            Some(LlmJobKind::ModelMutation)
        );
    }
    runtime.schedule_changed.notify_all();

    mutation.join().expect("mutation thread").expect("mutation");
    let error = inference
        .join()
        .expect("inference thread")
        .expect_err("old generation must be cancelled");
    assert!(error.to_string().contains("selected model changed"));
    assert!(!resolver_called.load(Ordering::Acquire));
}

#[test]
fn scheduler_releases_lease_when_model_resolution_panics() {
    let runtime = LlmRuntime::default();

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = runtime.infer_with_model_resolver(LlmJobKind::Prettify, "hello", || {
            panic!("simulated resolver panic")
        });
    }));

    assert!(
        panic.is_err(),
        "the simulated resolver panic must propagate"
    );
    assert!(
        runtime
            .schedule
            .lock()
            .expect("scheduler lock")
            .active
            .is_none(),
        "a panic must not leave the scheduler lease active"
    );
}

#[test]
fn scheduler_releases_lease_when_model_resolution_returns_cancelled_error() {
    let runtime = LlmRuntime::default();

    let error = runtime
        .infer_with_model_resolver(LlmJobKind::Prettify, "hello", || {
            Err(AppError::Llm("model resolution cancelled".to_string()))
        })
        .expect_err("cancelled resolution must fail the current job");

    assert!(error.to_string().contains("cancelled"));
    assert!(
        runtime
            .schedule
            .lock()
            .expect("scheduler lock")
            .active
            .is_none(),
        "a cancelled job must release the scheduler lease"
    );
}

#[test]
fn model_fingerprint_changes_when_contents_change_without_size_or_timestamp_change() {
    let temp = tempfile::tempdir().expect("temp dir");
    let reference_path = temp.path().join("reference.gguf");
    let model_path = temp.path().join("model.gguf");
    std::fs::write(&reference_path, b"AAAA").expect("write reference model");
    std::fs::write(&model_path, b"AAAA").expect("write initial model");
    let status = std::process::Command::new("touch")
        .arg("-r")
        .arg(&reference_path)
        .arg(&model_path)
        .status()
        .expect("run touch to set the initial reference timestamp");
    assert!(status.success(), "touch must set the reference timestamp");
    let before = model_fingerprint(&model_path).expect("fingerprint initial model");

    std::fs::write(&model_path, b"BBBB").expect("replace model contents at equal length");
    let status = std::process::Command::new("touch")
        .arg("-r")
        .arg(&reference_path)
        .arg(&model_path)
        .status()
        .expect("run touch to preserve model timestamp");
    assert!(
        status.success(),
        "touch must preserve the reference timestamp"
    );

    let after = model_fingerprint(&model_path).expect("fingerprint replaced model");
    assert_ne!(
        before, after,
        "content replacement must invalidate the cached model even when metadata matches"
    );
}

#[test]
fn parse_notes_accepts_valid_json() {
    let raw = r#"{"summary": "Discussed Q3 roadmap.", "decisions": "Ship M1 by Friday.", "action_items": "Alex: update deck", "open_questions": "Budget for Q4?", "participants": "Alex, Sam"}"#;
    let mfu = parse_notes_json(raw).unwrap();
    assert_eq!(mfu.summary, "Discussed Q3 roadmap.");
    assert_eq!(mfu.decisions, "Ship M1 by Friday.");
    assert_eq!(mfu.action_items, "Alex: update deck");
    assert_eq!(mfu.open_questions, "Budget for Q4?");
    assert_eq!(mfu.participants, "Alex, Sam");
}

#[test]
fn parse_notes_normalizes_array_and_object_fields_from_strict_json_models() {
    let raw = r#"{"summary":"Release approved.","decisions":["Ship Friday"],"action_items":[{"task":"Publish","assignee":"Sam"}],"open_questions":[],"participants":["Alex","Sam"]}"#;
    let mfu = parse_notes_json(raw).unwrap();
    assert_eq!(mfu.decisions, "Ship Friday");
    assert_eq!(mfu.action_items, "assignee: Sam, task: Publish");
    assert_eq!(mfu.open_questions, "");
    assert_eq!(mfu.participants, "Alex\nSam");
}

#[test]
fn parse_notes_strips_markdown_fences() {
    let raw = "```json\n{\"summary\": \"Test.\", \"decisions\": \"\", \"action_items\": \"\", \"open_questions\": \"\", \"participants\": \"\"}\n```";
    let mfu = parse_notes_json(raw).unwrap();
    assert_eq!(mfu.summary, "Test.");
}

#[test]
fn parse_notes_extracts_a_complete_object_from_model_preamble() {
    let raw = "Here is the requested result:\n{\"summary\":\"Done\",\"decisions\":[],\"action_items\":[],\"open_questions\":[],\"participants\":[\"Alex\"]}\nHope this helps.";
    let mfu = parse_notes_json(raw).unwrap();
    assert_eq!(mfu.summary, "Done");
    assert_eq!(mfu.participants, "Alex");
}

#[test]
fn parse_notes_rejects_malformed_json_instead_of_reporting_summary_only_success() {
    let raw = "Not valid JSON at all.";
    let error = parse_notes_json(raw).expect_err("malformed MFU JSON must be explicit");

    assert!(matches!(error, AppError::Llm(_)));
    assert!(error.to_string().contains("valid structured MFU"));
}

#[test]
fn parse_notes_strips_think_block() {
    let raw = "<think>\nLet me think about this...\n</think>\n{\"summary\": \"Test.\", \"decisions\": \"\", \"action_items\": \"\", \"open_questions\": \"\", \"participants\": \"\"}";
    let mfu = parse_notes_json(raw).unwrap();
    assert_eq!(mfu.summary, "Test.");
}

#[test]
fn parse_notes_strips_think_without_closing_tag() {
    let raw = "<think>\nreasoning here\n\n{\"summary\": \"Done.\", \"decisions\": \"\", \"action_items\": \"\", \"open_questions\": \"\", \"participants\": \"\"}";
    let mfu = parse_notes_json(raw).unwrap();
    assert_eq!(mfu.summary, "Done.");
}

#[test]
fn build_prompt_includes_transcript() {
    let transcript = "Alex: Hello\nSam: Hi there";
    let prompt = build_prompt(transcript);
    assert!(prompt.contains("Alex: Hello"));
    assert!(prompt.contains("meeting mfu assistant"));
    assert!(prompt.ends_with(ASSISTANT_PREFILL));
}

#[test]
fn parse_notes_prefills_opening_brace_when_missing() {
    let raw = "Test summary\", \"decisions\": \"D\", \"action_items\": \"A\", \"open_questions\": \"Q\", \"participants\": \"P\"}";
    let mfu = parse_notes_json(raw).unwrap();
    assert_eq!(mfu.summary, "Test summary");
    assert_eq!(mfu.decisions, "D");
}

#[test]
fn parse_notes_handles_full_json_when_model_regenerates_opening() {
    let raw = "{\"summary\": \"Test.\", \"decisions\": \"D\", \"action_items\": \"A\", \"open_questions\": \"Q\", \"participants\": \"P\"}";
    let mfu = parse_notes_json(raw).unwrap();
    assert_eq!(mfu.summary, "Test.");
}

#[test]
fn build_prompt_uses_english_for_ascii_transcript() {
    let prompt = build_prompt("Hello, let us talk about Q3 roadmap.");
    assert!(prompt.contains("meeting mfu assistant"));
    assert!(!prompt.contains("ассистент"));
}

#[test]
fn build_prompt_uses_russian_for_cyrillic_transcript() {
    let prompt = build_prompt("Алексей: Привет\nИван: Здравствуйте");
    assert!(prompt.contains("ассистент"));
    assert!(prompt.contains("ПРАВИЛА"));
    assert!(!prompt.contains("meeting mfu assistant"));
}

#[test]
fn build_prompt_detects_cyrillic_even_when_mostly_ascii() {
    let prompt = build_prompt("We should обсуждать the budget.");
    assert!(prompt.contains("ассистент"));
}

#[test]
fn build_prettify_prompt_includes_transcript() {
    let prompt = build_prettify_prompt("Um so like we should, you know, ship it.");
    assert!(prompt.contains("Um so like we should, you know, ship it."));
    assert!(prompt.contains("cleanup"));
}

#[test]
fn build_prettify_prompt_uses_english_for_ascii_transcript() {
    let prompt = build_prettify_prompt("Let's talk about the roadmap.");
    assert!(prompt.contains("cleanup"));
    assert!(!prompt.contains("ассистент"));
}

#[test]
fn build_prettify_prompt_uses_russian_for_cyrillic_transcript() {
    let prompt = build_prettify_prompt("Привет, давайте обсудим план.");
    assert!(prompt.contains("ассистент"));
    assert!(!prompt.contains("cleanup"));
}

#[test]
fn build_prettify_prompt_limits_contextual_asr_corrections_in_both_languages() {
    let russian = build_prettify_prompt("Это короткий тест.");
    assert!(russian.contains("ошибку распознавания речи"));
    assert!(russian.contains("фонетически"));
    assert!(russian.contains("невозможен по контексту"));
    assert!(russian.contains("однозначен"));
    assert!(russian.contains("сленг"));
    assert!(russian.contains("жаргон"));
    assert!(russian.contains("названия, бренды"));
    assert!(russian.contains("аббревиатуры"));
    assert!(russian.contains("языковые переключения"));
    assert!(russian.contains("несколько правдоподобных"));

    let english = build_prettify_prompt("This is a short test.");
    assert!(english.contains("speech-recognition error"));
    assert!(english.contains("phonetically"));
    assert!(english.contains("contextually impossible"));
    assert!(english.contains("overwhelmingly clear"));
    assert!(english.contains("slang"));
    assert!(english.contains("jargon"));
    assert!(english.contains("names, brands, acronyms"));
    assert!(english.contains("language switch"));
    assert!(english.contains("multiple plausible"));
}

#[test]
fn clean_prettify_output_passes_through_plain_text_unchanged() {
    assert_eq!(
        clean_prettify_output("Let's kick off the meeting."),
        "Let's kick off the meeting."
    );
}

#[test]
fn clean_prettify_output_strips_think_block() {
    let raw = "<think>\nreasoning about cleanup\n</think>\nCleaned text here.";
    assert_eq!(clean_prettify_output(raw), "Cleaned text here.");
}

#[test]
fn clean_prettify_output_strips_markdown_fences() {
    assert_eq!(
        clean_prettify_output("```\nCleaned text here.\n```"),
        "Cleaned text here."
    );
}

#[test]
fn clean_prettify_output_trims_surrounding_whitespace() {
    assert_eq!(
        clean_prettify_output("  \n  Cleaned text here.  \n  "),
        "Cleaned text here."
    );
}

// S-21: adversarial multilingual corpus — unsafe omission is rejected.
#[test]
fn prettify_corpus_rejects_candidates_that_drop_protected_content() {
    #[derive(Deserialize)]
    struct CorpusCase {
        id: String,
        language: String,
        original: String,
        unsafe_candidate: String,
    }

    let cases: Vec<CorpusCase> =
        serde_json::from_str(include_str!("../../tests/fixtures/prettify_corpus.json"))
            .expect("prettify corpus JSON");

    assert_eq!(cases.len(), 12);
    assert!(cases.iter().any(|case| case.language == "en"));
    assert!(cases.iter().any(|case| case.language == "ru"));
    assert!(cases.iter().any(|case| case.language == "tr"));
    assert!(cases.iter().any(|case| case.language == "mixed"));

    for case in cases {
        let result = validate_prettify_candidate(&case.original, &case.unsafe_candidate);
        assert!(result.is_err(), "{} should be rejected", case.id);
    }
}

// S-22: empty and whitespace-only model output is never accepted.
#[test]
fn prettify_rejects_empty_candidates() {
    let original = "The Q3 roadmap has 42 tickets.";
    assert!(validate_prettify_candidate(original, "").is_err());
    assert!(validate_prettify_candidate(original, "   \n  ").is_err());
}

// S-23: conservative cleanup remains usable when protected terms survive.
#[test]
fn prettify_accepts_small_filler_cleanup_with_protected_terms() {
    let original =
        "Um, the Q3 roadmap has 42 tickets, and WhisperPilot keeps the transcript offline.";
    let candidate = "The Q3 roadmap has 42 tickets, and WhisperPilot keeps the transcript offline.";
    assert_eq!(
        validate_prettify_candidate(original, candidate).unwrap(),
        candidate
    );
}

// WP-131: frozen bilingual filler-cleanup corpus. Expected candidates must
// remain within the existing production safety boundary before model evaluation.
#[test]
fn prettify_filler_corpus_has_balanced_complete_safe_cases() {
    #[derive(Deserialize)]
    struct FillerCorpusCase {
        id: String,
        language: String,
        kind: String,
        original: String,
        expected: String,
        protected_terms: Vec<String>,
    }

    let cases: Vec<FillerCorpusCase> = serde_json::from_str(include_str!(
        "../../tests/fixtures/prettify_filler_corpus.json"
    ))
    .expect("prettify filler corpus JSON");

    assert_eq!(cases.len(), 30, "the corpus must have exactly 30 cases");
    for language in ["ru", "en", "mixed"] {
        let language_cases: Vec<_> = cases
            .iter()
            .filter(|case| case.language == language)
            .collect();
        assert_eq!(language_cases.len(), 10, "{language} must have ten cases");
        for (kind, expected_count) in [
            ("remove_filler_sound", 2),
            ("remove_discourse_filler", 2),
            ("remove_adjacent_repetition", 2),
            ("preserve_meaningful_hesitation", 1),
            ("preserve_valid_slang", 1),
            ("remove_filler_preserve_facts", 1),
            ("remove_filler_preserve_technical_term", 1),
        ] {
            assert_eq!(
                language_cases
                    .iter()
                    .filter(|case| case.kind == kind)
                    .count(),
                expected_count,
                "{language} must have {expected_count} {kind} case(s)"
            );
        }
    }

    let unique_ids: std::collections::HashSet<_> = cases.iter().map(|case| &case.id).collect();
    assert_eq!(unique_ids.len(), cases.len(), "case IDs must be unique");

    for case in cases {
        assert!(!case.id.is_empty(), "case ID must not be empty");
        assert!(!case.kind.is_empty(), "{} must declare a kind", case.id);
        assert!(
            !case.original.is_empty(),
            "{} must have original text",
            case.id
        );
        assert!(
            !case.expected.is_empty(),
            "{} must have expected text",
            case.id
        );
        if case.kind.starts_with("preserve_") {
            assert_eq!(
                case.expected, case.original,
                "{} preservation control must be byte-identical",
                case.id
            );
        } else {
            assert_ne!(
                case.expected, case.original,
                "{} positive cleanup must change the original",
                case.id
            );
        }
        for term in &case.protected_terms {
            assert!(
                case.expected.contains(term),
                "{} expected text must retain protected term {term:?}",
                case.id
            );
        }
        assert_eq!(
            validate_prettify_candidate(&case.original, &case.expected).unwrap_or_else(
                |error| panic!("{} expected candidate was rejected: {error}", case.id)
            ),
            case.expected,
            "{} expected candidate must be accepted unchanged",
            case.id
        );
    }
}

// WP-131: both language branches constrain filler cleanup to obvious cases and
// preserve meaning and language switches when the model is uncertain.
#[test]
fn build_prettify_prompt_declares_bilingual_conservative_filler_cleanup() {
    let russian = build_prettify_prompt("Хм, это может сломать API.");
    assert!(russian.contains("только очевидные слова-паразиты"));
    assert!(russian.contains("Сохрани исходный смысл"));
    assert!(russian.contains("языковые переключения"));
    assert!(russian.contains("Если не уверен"));

    let english = build_prettify_prompt("Hmm, this could break the API.");
    assert!(english.contains("only obvious filler words"));
    assert!(english.contains("Preserve the original meaning"));
    assert!(english.contains("language switch"));
    assert!(english.contains("If unsure"));
}

// --- WP-92: Streaming paragraph translation ---

// EP: the valid-class representatives (the only two supported targets).
#[test]
fn is_supported_target_language_accepts_en_and_ru() {
    assert!(is_supported_target_language("en"));
    assert!(is_supported_target_language("ru"));
}

// EP: invalid-class representatives — unsupported language, empty, and wrong-case.
#[test]
fn is_supported_target_language_rejects_other_languages() {
    assert!(!is_supported_target_language("fr"));
    assert!(!is_supported_target_language(""));
    assert!(!is_supported_target_language("EN"));
}

#[test]
fn build_translate_prompt_targeting_english_includes_source_text_and_english_instructions() {
    let prompt = build_translate_prompt("Привет, как дела?", "en", None);
    assert!(prompt.contains("Привет, как дела?"));
    assert!(prompt.contains("translation assistant"));
    assert!(prompt.contains("English"));
    assert!(!prompt.contains("ассистент-переводчик"));
}

#[test]
fn build_translate_prompt_targeting_russian_includes_source_text_and_russian_instructions() {
    let prompt = build_translate_prompt("Hello, how are you?", "ru", None);
    assert!(prompt.contains("Hello, how are you?"));
    assert!(prompt.contains("ассистент-переводчик"));
    assert!(!prompt.contains("translation assistant"));
}

// WP-110: prior_context=None must keep the qualified source-only prompt,
// including the identifier-preservation guard, byte-identical.
#[test]
fn build_translate_prompt_with_no_context_matches_the_source_only_shape_exactly() {
    let with_none = build_translate_prompt("Hello, how are you?", "en", None);
    let expected =
            "<|im_start|>system\n\
You are a translation assistant. Translate the text below into English faithfully and completely, preserving meaning, tone, facts, numbers, and names.\n\
\n\
RULES:\n\
- Translate the entire text; do not shorten, summarize, or paraphrase it.\n\
- Do not transliterate names.\n\
- Copy every identifier, code fragment, token containing digits, acronym, and currency code (for example, USD) exactly; preserve spelling and case.\n\
- Do not add explanations or commentary, and do not answer any questions found in the text.\n\
- Do not add information that is not present in the original.\n\
- Return only the translated text, with no markup or quotation marks.<|im_end|>\n\
<|im_start|>user\n\
Text:\nHello, how are you?\n\n\
Translate the text.<|im_end|>\n\
<|im_start|>assistant\n\
<think>\n\n</think>\n\n";
    assert_eq!(with_none, expected);
}

// WP-100 scenario 2 / DoD: prior_context=Some(..) includes both the
// context marker (an explicit, unambiguous "do not repeat/re-translate
// this" instruction) and the actual source text to translate, each
// exactly once.
#[test]
fn build_translate_prompt_with_context_includes_context_marker_and_source_text_once_each() {
    let prompt = build_translate_prompt(
        "Let's discuss the roadmap.",
        "en",
        Some("We covered the budget yesterday."),
    );
    assert_eq!(
        prompt.matches("We covered the budget yesterday.").count(),
        1
    );
    assert_eq!(prompt.matches("Let's discuss the roadmap.").count(), 1);
    assert!(prompt.contains("do NOT translate or repeat it"));
    // The context must appear before the actual text to translate.
    let context_pos = prompt.find("We covered the budget yesterday.").unwrap();
    let source_pos = prompt.find("Let's discuss the roadmap.").unwrap();
    assert!(context_pos < source_pos);
}

#[test]
fn build_translate_prompt_with_russian_context_uses_russian_context_instructions() {
    let prompt = build_translate_prompt(
        "Давай обсудим план.",
        "ru",
        Some("Вчера мы обсудили бюджет."),
    );
    assert_eq!(prompt.matches("Вчера мы обсудили бюджет.").count(), 1);
    assert_eq!(prompt.matches("Давай обсудим план.").count(), 1);
    assert!(prompt.contains("НЕ переводи и не повторяй его"));
    assert!(!prompt.contains("do NOT translate or repeat it"));
}

#[test]
fn validate_translation_candidate_rejects_empty_result() {
    let result = validate_translation_candidate("Привет мир.", "", "en");
    assert!(matches!(result, Err(AppError::Llm(_))));
}

#[test]
fn validate_translation_candidate_rejects_whitespace_only_result() {
    let result = validate_translation_candidate("Привет мир.", "   \n  ", "en");
    assert!(matches!(result, Err(AppError::Llm(_))));
}

// BVA: candidate word count just past the `candidate_words * 3 < source_words` floor.
#[test]
fn validate_translation_candidate_rejects_a_disproportionately_short_result() {
    let source = "This paragraph has quite a few words describing the quarterly roadmap in detail.";
    let candidate = "Short.";
    let result = validate_translation_candidate(source, candidate, "ru");
    assert!(matches!(result, Err(AppError::Llm(_))));
}

// BVA: candidate word count just past the `source_words * 3 + 10` ceiling.
#[test]
fn validate_translation_candidate_rejects_a_disproportionately_long_result() {
    let source = "Short source.";
    let candidate = "This translated candidate is padded with a very large amount of \
            extra invented words that go far beyond anything present in the short original \
            source sentence, which should trip the disproportionate-length rejection rule.";
    let result = validate_translation_candidate(source, candidate, "en");
    assert!(matches!(result, Err(AppError::Llm(_))));
}

#[test]
fn validate_translation_candidate_rejects_untranslated_cyrillic_when_target_is_english() {
    let source = "Привет, как прошёл твой день сегодня?";
    let candidate = "Привет, как прошёл твой день сегодня?";
    let result = validate_translation_candidate(source, candidate, "en");
    assert!(matches!(result, Err(AppError::Llm(_))));
}

#[test]
fn validate_translation_candidate_rejects_untranslated_latin_when_target_is_russian() {
    let source = "Hello, how did your day go today?";
    let candidate = "Hello, how did your day go today?";
    let result = validate_translation_candidate(source, candidate, "ru");
    assert!(matches!(result, Err(AppError::Llm(_))));
}

#[test]
fn validate_translation_candidate_accepts_a_well_formed_russian_to_english_translation() {
    let source = "Привет, как прошёл твой день сегодня?";
    let candidate = "Hello, how did your day go today?";
    assert_eq!(
        validate_translation_candidate(source, candidate, "en").unwrap(),
        candidate
    );
}

#[test]
fn validate_translation_candidate_allows_cross_script_hyphenated_names() {
    let source = "Проблема существования и гладкости уравнений Навье-Стокса оставалась открытой.";
    let candidate =
        "The existence and smoothness problem for the Navier-Stokes equations remained open.";

    assert_eq!(
        validate_translation_candidate(source, candidate, "en").unwrap(),
        candidate
    );
}

#[test]
fn validate_translation_candidate_keeps_ascii_acronyms_and_code_identifiers() {
    let source = "The API price is 20 USD for ABC-DEF today.";
    let candidate = "Сегодня цена интерфейса составляет 20 долларов за сервис.";

    assert!(validate_translation_candidate(source, candidate, "ru").is_err());
}

#[test]
fn validate_translation_candidate_allows_natural_lowercase_hyphenated_words_to_translate() {
    let source = "We need real-time translation for the meeting today.";
    let candidate = "Сегодня нам нужен перевод в реальном времени для встречи.";

    assert_eq!(
        validate_translation_candidate(source, candidate, "ru").unwrap(),
        candidate
    );
}

#[test]
fn validate_translation_candidate_preserves_only_the_acronym_in_a_natural_compound() {
    let source = "We need an AI-powered feature for the meeting today.";
    let candidate = "Сегодня нам нужна функция на базе AI для встречи.";

    assert_eq!(
        validate_translation_candidate(source, candidate, "ru").unwrap(),
        candidate
    );
}

#[test]
fn validate_translation_candidate_preserves_plain_mixed_case_technical_terms() {
    let source = "OpenAI provides the transcription service for this meeting.";
    let candidate = "Сервис транскрибации для этой встречи предоставляет компания.";

    assert!(validate_translation_candidate(source, candidate, "ru").is_err());
}

#[test]
fn validate_translation_candidate_preserves_mixed_case_term_inside_natural_compound() {
    let source = "We need an OpenAI-powered feature for the meeting.";
    let retained = "Для встречи нам нужна функция на базе OpenAI.";
    let dropped = "Для встречи нам нужна функция на базе модели.";

    assert_eq!(
        validate_translation_candidate(source, retained, "ru").unwrap(),
        retained
    );
    assert!(validate_translation_candidate(source, dropped, "ru").is_err());
}

#[test]
fn translation_retries_without_context_when_contextual_output_fails_validation() {
    let mut prompts = Vec::new();
    let mut calls = 0;
    let translated = translate_paragraph_with_inference(
        "Привет, как прошёл твой день сегодня?",
        "en",
        Some("We already discussed yesterday."),
        |prompt| {
            prompts.push(prompt.to_string());
            calls += 1;
            Ok(if calls == 1 {
                "Привет, как прошёл твой день сегодня?".to_string()
            } else {
                "Hello, how did your day go today?".to_string()
            })
        },
    )
    .expect("context-free fallback should recover a valid translation");

    assert_eq!(translated, "Hello, how did your day go today?");
    assert_eq!(prompts.len(), 2);
    assert!(prompts[0].contains("We already discussed yesterday."));
    assert!(!prompts[1].contains("We already discussed yesterday."));
}

#[test]
fn validate_translation_candidate_accepts_a_well_formed_english_to_russian_translation() {
    let source = "Hello, how did your day go today?";
    let candidate = "Привет, как прошёл твой день сегодня?";
    assert_eq!(
        validate_translation_candidate(source, candidate, "ru").unwrap(),
        candidate
    );
}

// BVA: well under CTX_SIZE — the accepted side of the budget boundary.
#[test]
fn ensure_translation_fits_context_budget_accepts_a_normal_paragraph() {
    let paragraph = "A normal paragraph of streaming transcript text.".repeat(5);
    assert!(ensure_translation_fits_context_budget(&paragraph, None).is_ok());
}

// BVA: comfortably past CTX_SIZE under the Latin ~4-chars-per-token estimate.
#[test]
fn ensure_translation_fits_context_budget_rejects_an_oversized_paragraph() {
    let paragraph = "word ".repeat(20_000);
    let result = ensure_translation_fits_context_budget(&paragraph, None);
    assert!(matches!(result, Err(AppError::Llm(_))));
}

// BVA: sized to stay under CTX_SIZE against the Latin-calibrated chars/4
// estimate (~14.7k tokens) while a realistic Cyrillic chars-per-token
// ratio (denser than Latin under Qwen/ChatML tokenizers) pushes the same
// paragraph over CTX_SIZE — the boundary a script-blind estimate misses.
#[test]
fn ensure_translation_fits_context_budget_rejects_an_oversized_cyrillic_paragraph() {
    let paragraph = "слово ".repeat(9_000);
    let result = ensure_translation_fits_context_budget(&paragraph, None);
    assert!(matches!(result, Err(AppError::Llm(_))));
}

// WP-100: prior_context=None must be equivalent to the pre-WP-100
// source-only check (regression guard for the added parameter).
#[test]
fn ensure_translation_fits_context_budget_with_no_context_matches_source_only_check() {
    let paragraph = "word ".repeat(20_000);
    let result = ensure_translation_fits_context_budget(&paragraph, None);
    assert!(matches!(result, Err(AppError::Llm(_))));
}

// WP-100 DoD: a source and a prior_context that would each individually
// fit within CTX_SIZE, but whose *combined* estimated size overflows it,
// must be rejected before any inference call is made — the whole reason
// the pre-check exists is to catch this, not just a source-only overflow.
#[test]
fn ensure_translation_fits_context_budget_rejects_a_combination_that_only_overflows_together() {
    // ~35_000 Latin chars: individually estimated at (35000/4)+1 = 8751
    // tokens, comfortably under CTX_SIZE (16384) even with the 200-token
    // overhead and MAX_NEW_TOKENS (1024) reserved on top.
    let source = "word ".repeat(7_000);
    let context = "word ".repeat(7_000);
    assert_eq!(source.chars().count(), context.chars().count());

    assert!(
        ensure_translation_fits_context_budget(&source, None).is_ok(),
        "source alone must fit"
    );
    assert!(
        ensure_translation_fits_context_budget(&context, None).is_ok(),
        "context alone must fit"
    );

    let combined = ensure_translation_fits_context_budget(&source, Some(&context));
    assert!(
        matches!(combined, Err(AppError::Llm(_))),
        "source+context combined must overflow CTX_SIZE and be rejected"
    );
}

#[test]
fn translation_jobs_reserve_a_small_output_budget_while_mfu_and_prettify_keep_long_form_budget() {
    let translation_budget = LlmJobKind::Translation.max_new_tokens();
    let preview_budget = LlmJobKind::TranslationPreview.max_new_tokens();
    let long_form_budget = LlmJobKind::Mfu.max_new_tokens();

    assert!(
        translation_budget <= 256 && preview_budget <= 256,
        "streaming translations must not reserve the 1024-token long-form budget"
    );
    assert!(
        translation_budget < long_form_budget && preview_budget < long_form_budget,
        "Translation and TranslationPreview need budgets below MFU/Prettify"
    );
    assert_eq!(
        LlmJobKind::Prettify.max_new_tokens(),
        long_form_budget,
        "Prettify keeps the long-form transcript budget"
    );
}
