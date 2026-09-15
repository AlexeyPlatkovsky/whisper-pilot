use super::policy::first_complete_json_object;
use super::runtime::{LoadedLlmModel, CTX_SIZE, N_BATCH};
use super::*;

struct ChatPrompt<'a> {
    system: &'a str,
    user: &'a str,
    assistant_prefill: &'a str,
}

fn parse_chatml_prompt(prompt: &str) -> Option<ChatPrompt<'_>> {
    const SYSTEM: &str = "<|im_start|>system\n";
    const SYSTEM_TO_USER: &str = "<|im_end|>\n<|im_start|>user\n";
    const USER_TO_ASSISTANT: &str = "<|im_end|>\n<|im_start|>assistant\n";
    let after_system = prompt.strip_prefix(SYSTEM)?;
    let (system, user_and_assistant) = after_system.split_once(SYSTEM_TO_USER)?;
    let (user, assistant_prefill) = user_and_assistant.rsplit_once(USER_TO_ASSISTANT)?;
    Some(ChatPrompt {
        system,
        user,
        assistant_prefill,
    })
}

fn prepare_prompt(loaded: &LoadedLlmModel, raw: &str) -> Result<(String, AddBos)> {
    let Some(spec) = loaded.spec else {
        return Ok((raw.to_string(), AddBos::Always));
    };
    let Some(chat) = parse_chatml_prompt(raw) else {
        return Ok((raw.to_string(), AddBos::Always));
    };
    if spec.chat_template == "gemma4-canonical-no-think" {
        let mut prompt = format!(
            "<|turn>system\n{}<turn|>\n<|turn>user\n{}<turn|>\n<|turn>model\n<|channel>thought\n<channel|>",
            chat.system.trim(),
            chat.user.trim()
        );
        let prefill = chat
            .assistant_prefill
            .trim_start()
            .strip_prefix("<think>\n\n</think>")
            .unwrap_or(chat.assistant_prefill.trim_start())
            .trim_start();
        prompt.push_str(prefill);
        return Ok((prompt, AddBos::Always));
    }
    let template = if spec.chat_template == "embedded" {
        loaded
            .model
            .chat_template(None)
            .map_err(|error| AppError::Llm(format!("embedded chat template: {error}")))?
    } else {
        LlamaChatTemplate::new(spec.chat_template)
            .map_err(|error| AppError::Llm(format!("pinned chat template: {error}")))?
    };
    let user_content = if spec.template_family == "qwen" && spec.chat_template != "embedded" {
        format!("{}\n\n/no_think", chat.user)
    } else {
        chat.user.to_string()
    };
    let messages = [
        LlamaChatMessage::new("system".into(), chat.system.into())
            .map_err(|error| AppError::Llm(format!("system chat message: {error}")))?,
        LlamaChatMessage::new("user".into(), user_content)
            .map_err(|error| AppError::Llm(format!("user chat message: {error}")))?,
    ];
    let mut prompt = match loaded.model.apply_chat_template(&template, &messages, true) {
        Ok(prompt) => prompt,
        Err(embedded_error) => {
            // llama.cpp's compact C template renderer intentionally supports
            // only common template families. Some GGUFs embed full Jinja
            // (Gemma 4 does), so use the pinned compatible family instead of
            // rejecting an otherwise supported model or inventing raw tokens.
            let fallback = LlamaChatTemplate::new(spec.template_family)
                .map_err(|error| AppError::Llm(format!("fallback chat template: {error}")))?;
            loaded
                .model
                .apply_chat_template(&fallback, &messages, true)
                .map_err(|fallback_error| {
                    AppError::Llm(format!(
                        "apply embedded chat template: {embedded_error}; fallback {}: {fallback_error}",
                        spec.template_family
                    ))
                })?
        }
    };
    if spec.template_family != "gemma" {
        prompt.push_str(chat.assistant_prefill.trim_start());
    }
    let add_bos = if spec.template_family == "gemma" {
        AddBos::Always
    } else {
        AddBos::Never
    };
    Ok((prompt, add_bos))
}

fn sampler_for(spec: Option<&LlmModelSpec>) -> LlamaSampler {
    match spec {
        Some(spec) if spec.temperature > 0.0 => LlamaSampler::chain_simple([
            LlamaSampler::top_k(spec.top_k),
            LlamaSampler::top_p(spec.top_p, 1),
            LlamaSampler::temp(spec.temperature),
            LlamaSampler::dist(0x5750_4c54),
        ]),
        _ => LlamaSampler::greedy(),
    }
}

pub(crate) fn run_inference_with_model(
    loaded: &LoadedLlmModel,
    kind: LlmJobKind,
    prompt: &str,
) -> Result<String> {
    let max_new_tokens = kind.max_new_tokens();
    let (prompt, add_bos) = prepare_prompt(loaded, prompt)?;
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4);
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(CTX_SIZE))
        .with_n_batch(N_BATCH)
        .with_n_threads(n_threads);

    let mut ctx = loaded
        .model
        .new_context(loaded.backend.get()?, ctx_params)
        .map_err(|e| AppError::Llm(format!("context create: {e}")))?;

    let tokens_list = loaded
        .model
        .str_to_token(&prompt, add_bos)
        .map_err(|e| AppError::Llm(format!("tokenize: {e}")))?;

    run_with_token_preflight(
        tokens_list.len(),
        max_new_tokens as usize,
        CTX_SIZE as usize,
        || Ok(()),
    )?;

    let total = tokens_list.len();
    let chunk_size = N_BATCH as usize;
    let mut n_cur: i32 = 0;
    let mut batch = LlamaBatch::new(chunk_size, 1);

    for chunk_start in (0..total).step_by(chunk_size) {
        let chunk_end = (chunk_start + chunk_size).min(total);
        let is_last_chunk = chunk_end == total;

        batch.clear();
        for (offset, &token) in tokens_list[chunk_start..chunk_end].iter().enumerate() {
            let pos = n_cur + offset as i32;
            let logits = is_last_chunk && offset == chunk_end - chunk_start - 1;
            batch
                .add(token, pos, &[0], logits)
                .map_err(|e| AppError::Llm(format!("batch add: {e}")))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| AppError::Llm(format!("decode: {e}")))?;
        n_cur += (chunk_end - chunk_start) as i32;
    }

    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut sampler = sampler_for(loaded.spec);
    let mut output = String::new();

    while n_cur - (total as i32) < max_new_tokens {
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);

        sampler.accept(token);

        if loaded.model.is_eog_token(token) {
            break;
        }

        match loaded.model.token_to_piece(token, &mut decoder, true, None) {
            Ok(piece) => output.push_str(&piece),
            Err(e) => return Err(AppError::Llm(format!("token to piece: {e}"))),
        }

        if matches!(kind, LlmJobKind::Mfu) && first_complete_json_object(&output).is_some() {
            break;
        }
        if ["<|im_end|>", "<end_of_turn>", "<|endoftext|>"]
            .iter()
            .any(|stop| output.contains(stop))
        {
            break;
        }

        batch.clear();
        batch
            .add(token, n_cur, &[0], true)
            .map_err(|e| AppError::Llm(format!("batch add: {e}")))?;

        n_cur += 1;

        ctx.decode(&mut batch)
            .map_err(|e| AppError::Llm(format!("decode: {e}")))?;
    }

    Ok(output)
}
