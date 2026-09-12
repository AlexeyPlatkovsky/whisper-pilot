use whisperpilot_lib::llm::strip_internal_reasoning;
use whisperpilot_lib::models::{llm_spec_by_id, LlmProfile};

#[test]
fn phase_three_profiles_are_explicit_and_keep_legacy_models_addressable() {
    let fast = llm_spec_by_id("qwen3.5-4b-q4km").expect("Fast model spec");
    assert_eq!(fast.profile, LlmProfile::Fast);
    assert!(fast.recommended);
    assert_eq!(fast.context_tokens, 16_384);

    for id in ["qwen3.8-9b-q6k", "gemma4-12b-q4"] {
        let quality = llm_spec_by_id(id).expect("Quality model spec");
        assert_eq!(quality.profile, LlmProfile::Quality);
        assert!(!quality.recommended);
    }

    assert!(llm_spec_by_id("qwen3-4b-q3kl").is_some());
}

#[test]
fn all_visible_text_strips_reasoning_and_model_control_tokens() {
    assert_eq!(
        strip_internal_reasoning(
            "<think>private chain</think>\nПолезный ответ<|im_end|><end_of_turn>"
        ),
        "Полезный ответ"
    );
    assert_eq!(
        strip_internal_reasoning("```text\nUseful answer\n```"),
        "Useful answer"
    );
}
