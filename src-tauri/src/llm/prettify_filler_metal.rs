use super::policy::build_prettify_prompt;
use super::{strip_internal_reasoning, LlmJobKind, LlmRuntime};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const MODEL_ENV: &str = "WHISPERPILOT_TEST_LLM_MODEL";
const FAST_MODEL_FILENAME: &str = "Qwen3.5-4B-Q4_K_M.gguf";
const FAST_MODEL_SHA256: &str = "00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4";

#[derive(Deserialize)]
struct Case {
    id: String,
    language: String,
    kind: String,
    original: String,
    expected: String,
    protected_terms: Vec<String>,
}

#[derive(Default)]
struct Score {
    completed: usize,
    exact: usize,
    positive_exact: usize,
    preservation_exact: usize,
    critical: Vec<String>,
    errors: Vec<String>,
}

impl Score {
    fn meets_filler_gate(&self) -> bool {
        // Full adoption also requires the separate >=14/30 ASR-corpus gate.
        self.exact >= 26
            && self.positive_exact >= 20
            && self.preservation_exact == 6
            && self.critical.is_empty()
            && self.errors.is_empty()
    }
}

fn cases() -> Vec<Case> {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/prettify_filler_corpus.json"
    ))
    .expect("prettify filler corpus JSON")
}

fn model_path() -> String {
    let model = std::env::var(MODEL_ENV)
        .unwrap_or_else(|_| panic!("set {MODEL_ENV} to the downloaded Qwen3.5 Fast GGUF path"));
    assert_eq!(
        Path::new(&model).file_name().and_then(|name| name.to_str()),
        Some(FAST_MODEL_FILENAME),
        "the WP-131 qualification profile must be Qwen3.5 Fast"
    );
    let mut file = File::open(&model).expect("open Qwen3.5 Fast GGUF");
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).expect("hash Qwen3.5 Fast GGUF");
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    assert_eq!(
        format!("{:x}", hasher.finalize()),
        FAST_MODEL_SHA256,
        "the WP-131 qualification model must match the catalog artifact"
    );
    model
}

fn candidate_prompt(transcript: &str) -> String {
    let is_russian = transcript
        .chars()
        .any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
    let (system, lead) = if is_russian {
        (
            "Ты осторожно очищаешь ASR-расшифровку. Сохрани смысл, стиль, порядок и каждый корректный токен.\n\
\n\
УДАЛЯЙ ТОЛЬКО ЯВНЫЙ ШУМ РЕЧИ:\n\
- отдельные звуки паузы без смысловой роли: э, э-э, эээ, эм, ммм, а также uh, um, umm, er;\n\
- вводные слова-паразиты вроде «ну», «в общем», «типа», well, you know, like, so, только когда они синтаксически\n\
отделимы и ничего не означают в данном предложении;\n\
- один экземпляр очевидного случайного соседнего повтора.\n\
После удаления аккуратно убери оставшуюся запятую и восстанови заглавную букву первого слова. Сохраняй «хм»/hmm,\n\
когда оно выражает сомнение или реакцию. Сохраняй любое похожее на filler слово, если это имя, термин, подлежащее,\n\
часть устойчивой фразы или оно влияет на смысл. Цепочку нескольких начальных fillers можно удалить целиком.\n\
\n\
ИСПРАВЛЯЙ ТОЛЬКО ОЧЕВИДНЫЕ ASR-ОШИБКИ:\n\
Ищи разорванные, склеенные и фонетически искажённые слова или короткие фразы. Замени минимальный фрагмент, только если\n\
грамматика и тема оставляют один разумный термин. Точный термин из соседнего контекста имеет приоритет; согласуй только\n\
форму восстановленного термина.\n\
\n\
Не переводи корректные слова и не меняй переключения языка. Не меняй сленг, имена, бренды, команды, аббревиатуры, числа,\n\
версии и технические термины. Не перефразируй и не добавляй текст. При сомнении копируй исходный фрагмент.\n\
\n\
Примеры:\n\
Вход: Эм, мы, мы начнём сборку.\nВыход: Мы начнём сборку.\n\
Вход: Хм, это действительно рискованно.\nВыход: Хм, это действительно рискованно.\n\
Вход: Like — название флага, not a filler.\nВыход: Like — название флага, not a filler.\n\
Вход: Открой бра у зер и продолжай.\nВыход: Открой браузер и продолжай.\n\
\n\
Верни только полный очищенный текст без кавычек, меток, тегов и пояснений.",
            "Очисти эту расшифровку:\n",
        )
    } else {
        (
            "Conservatively clean an ASR transcript. Preserve meaning, style, order, and every valid token.\n\
\n\
REMOVE ONLY OBVIOUS SPEECH NOISE:\n\
- standalone hesitation sounds with no semantic role: uh, um, umm, er, and their transcription variants;\n\
- discourse fillers such as well, you know, like, so, or I mean only when syntactically detachable and meaningless in\n\
the specific sentence;\n\
- one copy of an obvious accidental adjacent repetition.\n\
After removal, clean up only the orphaned comma and capitalize the new first word. Preserve hmm when it expresses doubt or\n\
reaction. Preserve any filler-looking word when it is a name, term, subject, part of a fixed phrase, or affects meaning. A\n\
chain of several initial fillers may be removed together.\n\
\n\
CORRECT ONLY OBVIOUS ASR ERRORS:\n\
Look for split, merged, or phonetically corrupted words and short phrases. Replace the smallest span only when grammar and\n\
topic leave one reasonable term. An exact term in nearby context takes priority; adjust only the recovered term's form.\n\
\n\
Never translate valid words or change language switches. Do not change slang, names, brands, commands, acronyms, numbers,\n\
versions, or technical terms. Do not paraphrase or add text. When unsure, copy the original span.\n\
\n\
Examples:\n\
Input: Um, we, we will start the build.\nOutput: We will start the build.\n\
Input: Hmm, that is genuinely risky.\nOutput: Hmm, that is genuinely risky.\n\
Input: Well is the project name, not a filler.\nOutput: Well is the project name, not a filler.\n\
Input: Open the bra ow ser and continue.\nOutput: Open the browser and continue.\n\
\n\
Return only the complete cleaned text without quotes, labels, tags, or explanations.",
            "Clean this transcript:\n",
        )
    };

    format!(
        "<|im_start|>system\n{system}<|im_end|>\n\
<|im_start|>user\n{lead}{transcript}<|im_end|>\n\
<|im_start|>assistant\n<think>\n\n</think>\n\n"
    )
}

fn score_outputs(label: &str, cases: &[Case], outputs: Vec<Result<String, String>>) -> Score {
    let mut score = Score::default();
    for (case, result) in cases.iter().zip(outputs) {
        let output = match result {
            Ok(output) => output,
            Err(error) => {
                eprintln!("{label}\t{}\tERROR\t{error}", case.id);
                score.errors.push(format!("{}: {error}", case.id));
                continue;
            }
        };
        score.completed += 1;
        let exact = output == case.expected;
        let preservation = case.kind.starts_with("preserve_");
        score.exact += usize::from(exact);
        score.positive_exact += usize::from(exact && !preservation);
        score.preservation_exact += usize::from(exact && preservation);

        let output_tokens: Vec<_> = output
            .split_whitespace()
            .map(normalize_protected_token)
            .collect();
        let missing: Vec<_> = case
            .protected_terms
            .iter()
            .filter(|term| !output_tokens.contains(&normalize_protected_token(term)))
            .collect();
        if output != case.original && !exact {
            score.critical.push(format!(
                "{} produced a third, unapproved rendering",
                case.id
            ));
        }
        if !missing.is_empty() {
            score
                .critical
                .push(format!("{} lost protected terms {missing:?}", case.id));
        }
        if !exact {
            eprintln!(
                "{label}\t{}\t{}\t{}\nexpected: {:?}\nobserved: {:?}",
                case.id, case.language, case.kind, case.expected, output
            );
        }
    }
    eprintln!(
        "{label}: completed={}/30 exact={}/30 positive={}/24 preservation={}/6 critical={:?} errors={:?}",
        score.completed,
        score.exact,
        score.positive_exact,
        score.preservation_exact,
        score.critical,
        score.errors
    );
    score
}

fn normalize_protected_token(token: &str) -> &str {
    token
        .trim_matches(|character: char| {
            !character.is_alphanumeric()
                && character != '%'
                && character != '-'
                && character != '_'
                && character != '.'
        })
        .trim_end_matches('.')
}

// WP-131: opt-in because this loads a multi-GB GGUF and requires real macOS Metal.
// Run with: WHISPERPILOT_TEST_LLM_MODEL=/path/to/Qwen3.5-4B-Q4_K_M.gguf cargo test
// --manifest-path src-tauri/Cargo.toml --lib qwen_fast_compares -- --ignored --nocapture
#[test]
#[ignore = "requires a downloaded Qwen3.5 Fast GGUF and real macOS Metal"]
fn qwen_fast_compares_current_and_literal_few_shot_prompts() {
    let cases = cases();
    let model = model_path();
    let runtime = LlmRuntime::default();

    let current_outputs = cases
        .iter()
        .map(|case| {
            runtime
                .infer(
                    Path::new(&model),
                    LlmJobKind::Prettify,
                    &build_prettify_prompt(&case.original),
                )
                .map(|raw| strip_internal_reasoning(&raw))
                .map_err(|error| error.to_string())
        })
        .collect();
    let current = score_outputs("current", &cases, current_outputs);
    assert_eq!(current.completed, 30, "current inference must complete");
    assert!(current.errors.is_empty(), "current inference errors");
    assert_eq!(
        current.preservation_exact, 6,
        "current prompt must preserve all controls"
    );

    let candidate_outputs = cases
        .iter()
        .map(|case| {
            runtime
                .infer(
                    Path::new(&model),
                    LlmJobKind::Prettify,
                    &candidate_prompt(&case.original),
                )
                .map(|raw| strip_internal_reasoning(&raw))
                .map_err(|error| error.to_string())
        })
        .collect();
    let candidate = score_outputs("literal-few-shot", &cases, candidate_outputs);
    assert_eq!(candidate.completed, 30, "candidate inference must complete");
    assert!(candidate.errors.is_empty(), "candidate inference errors");

    assert!(!current.meets_filler_gate(), "current met the filler gate");
    assert!(
        !candidate.meets_filler_gate(),
        "candidate now meets the filler gate; run the separate ASR gate before adoption"
    );
}
