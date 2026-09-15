//! Local LLM orchestration.
//!
//! The module is intentionally split by responsibility: the scheduler/cache
//! runtime owns the single Metal-backed execution lane, inference owns llama
//! prompt execution, and policy modules own MFU/prettify and translation.

use crate::error::{AppError, Result};
use crate::models::catalog::LlmModelSpec;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaChatTemplate};
use llama_cpp_2::sampling::LlamaSampler;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::num::NonZeroU32;
use std::path::Path;

mod inference;
mod policy;
mod runtime;
mod translation;

pub use policy::{
    generate_mfu, generate_mfu_with_inference, generate_mfu_with_model_resolver,
    prettify_transcript, prettify_transcript_with_model_resolver, strip_internal_reasoning,
    GeneratedMfu,
};
pub(crate) use runtime::SharedLlamaBackend;
pub use runtime::{
    run_with_token_preflight, BoundedLlmScheduler, LlmJobKind, LlmRuntime, LlmScheduleError,
    ModelFingerprint, ScheduledLlmJob, SelectedModelCache,
};
pub use translation::{
    is_supported_target_language, preview_translation_with_model_resolver, translate_paragraph,
    translate_paragraph_with_model_resolver, SUPPORTED_TRANSLATION_TARGETS,
};

// Runtime-private re-exports keep the module boundaries explicit without
// widening the application's public LLM API.
pub(crate) use inference::run_inference_with_model;
pub(crate) use translation::ensure_prompt_fits_context_budget;

#[cfg(test)]
#[path = "llm/tests.rs"]
mod tests;
