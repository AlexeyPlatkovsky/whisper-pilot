//! Tauri app state: shared model runtimes and the single live audio source.

use crate::error::{AppError, Result};
use crate::live_capture::LiveCaptureRuntimeCoordinator;
use crate::llm;
#[cfg(target_os = "macos")]
use crate::microphone_audio;
#[cfg(target_os = "macos")]
use crate::streaming_audio;
use crate::transcribe;
#[cfg(target_os = "macos")]
use qwen_asr::context::QwenModel;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;
use tokio::sync::Mutex;
use whisper_rs::WhisperContext;

pub(crate) fn app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Io(e.to_string()))
}

pub(crate) fn now_ms() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| AppError::Store(error.to_string()))
        .and_then(|duration| {
            i64::try_from(duration.as_millis())
                .map_err(|_| AppError::Store("current time exceeds i64 milliseconds".into()))
        })
}

/// The whisper model is loaded lazily on first use and cached for the
/// session — loading ~800 MB should not block app launch.
///
/// `whisper_busy` enforces WP-71's mutual exclusion (Meeting transcription
/// vs. Streaming session, both contending for `model`'s one cached context)
/// via `streaming_session::WhisperUsageGuard`. It's a plain field, not
/// wrapped alongside `model`, so acquiring it stays synchronous without
/// holding `model`'s lock for the guard's whole lifetime.
#[derive(Default)]
pub(crate) struct AppState {
    pub(crate) model: Mutex<Option<Arc<WhisperContext>>>,
    #[cfg(target_os = "macos")]
    pub(crate) qwen_asr_model: Mutex<Option<QwenAsrCache>>,
    #[cfg(target_os = "macos")]
    pub(crate) qwen_gguf_asr_model: Mutex<Option<QwenGgufAsrCache>>,
    /// Serializes Recorder ASR selection, loading, and deletion. A Start holds
    /// this until its live-capture state is installed, so model deletion cannot
    /// pass an idle check and remove a bundle being loaded by that Start.
    pub(crate) recorder_asr_mutation: Mutex<()>,
    pub(crate) whisper_busy: std::sync::atomic::AtomicU8,
    /// One bounded local-LLM scheduler and selected-model cache for the app.
    /// App ownership ensures Metal resources are released during teardown.
    pub(crate) llm_runtime: Arc<llm::LlmRuntime>,
    /// Authoritative live-capture lifecycle and native runtime. Dropping the
    /// runtime stops system-audio capture and cascades through decode and
    /// persistence. Backend ownership survives navigation and webview
    /// remounts; see `docs/architecture.md`'s Streaming IPC section.
    #[cfg(target_os = "macos")]
    pub(crate) live_capture: StdMutex<LiveCaptureRuntimeCoordinator<LiveCaptureRuntime>>,
    #[cfg(not(target_os = "macos"))]
    pub(crate) live_capture: StdMutex<LiveCaptureRuntimeCoordinator<()>>,
    /// WP-92's single-flight guard for Streaming window translation: at
    /// most one `translate_streaming_window` call runs its LLM inference
    /// at a time, claimed via `llm::TranslationUsageGuard`. Independent of
    /// `whisper_busy` (a different shared resource, the llama.cpp model
    /// rather than the Whisper context) so translation never blocks or is
    /// blocked by the streaming decode loop.
    pub(crate) translation_busy: std::sync::atomic::AtomicBool,
    #[cfg(target_os = "macos")]
    pub(crate) recorder_shortcut_gate: StdMutex<crate::recorder_shortcut::RecorderShortcutGate>,
    #[cfg(target_os = "macos")]
    pub(crate) registered_recorder_shortcut:
        StdMutex<Option<tauri_plugin_global_shortcut::Shortcut>>,
    #[cfg(target_os = "macos")]
    pub(crate) recorder_shortcut_error: StdMutex<Option<String>>,
}

#[cfg(target_os = "macos")]
pub(crate) struct QwenAsrCache {
    key: String,
    model: Arc<QwenModel>,
}

#[cfg(target_os = "macos")]
pub(crate) struct QwenGgufAsrCache {
    key: String,
    model: Arc<crate::qwen_gguf_asr::QwenGgufAsrModel>,
}

// Both fields are held for their effect, not read back: `session_id`
// documents which session this runtime belongs to (useful reading a debug
// dump or extending this later); `capture`'s only job is to exist until
// `stop_streaming_session` drops it, which is what actually stops capture.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
pub(crate) struct StreamingRuntime {
    pub(crate) session_id: i64,
    pub(crate) capture: streaming_audio::StreamingSession,
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
pub(crate) struct RecorderRuntime {
    pub(crate) session_id: i64,
    pub(crate) capture: microphone_audio::MicrophoneCaptureSession,
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
pub(crate) enum LiveCaptureRuntime {
    Streaming(StreamingRuntime),
    Recorder(RecorderRuntime),
}

impl AppState {
    pub(crate) async fn model(&self, app_support_dir: PathBuf) -> Result<Arc<WhisperContext>> {
        let mut guard = self.model.lock().await;
        if let Some(ctx) = guard.as_ref() {
            return Ok(Arc::clone(ctx));
        }
        // Loading is blocking and CPU-heavy; keep it off the async reactor.
        let ctx = tokio::task::spawn_blocking(move || transcribe::load_model(&app_support_dir))
            .await
            .map_err(|e| AppError::ModelLoad(e.to_string()))??;
        let ctx = Arc::new(ctx);
        *guard = Some(Arc::clone(&ctx));
        Ok(ctx)
    }

    #[cfg(target_os = "macos")]
    pub(crate) async fn qwen_asr_model(
        &self,
        app_support_dir: PathBuf,
        spec: &'static crate::asr::AsrModelSpec,
    ) -> Result<Arc<QwenModel>> {
        let entry = crate::models::CATALOG
            .iter()
            .find(|entry| entry.id == spec.model_id)
            .ok_or_else(|| {
                AppError::ModelLoad(format!("missing catalog entry {}", spec.model_id))
            })?;
        if !entry
            .assets
            .iter()
            .all(|asset| crate::models::catalog::is_asset_downloaded(&app_support_dir, asset))
        {
            return Err(AppError::ModelNotFound(format!(
                "complete verified {} bundle; download it in Settings → AI Models",
                spec.model_id
            )));
        }
        let key = format!(
            "{}:{}:{}",
            spec.engine.as_str(),
            spec.model_id,
            spec.asset_fingerprint
        );
        let mut guard = self.qwen_asr_model.lock().await;
        if let Some(cache) = guard.as_ref().filter(|cache| cache.key == key) {
            return Ok(Arc::clone(&cache.model));
        }
        let model_dir = crate::models::asset_paths(&app_support_dir, spec.model_id)
            .and_then(|paths| paths.into_iter().next())
            .and_then(|path| path.parent().map(PathBuf::from))
            .ok_or_else(|| AppError::ModelLoad("Qwen3-ASR bundle directory is invalid".into()))?;
        let model = tokio::task::spawn_blocking(move || {
            let path = model_dir
                .to_str()
                .ok_or_else(|| AppError::ModelLoad("Qwen3-ASR path is not valid UTF-8".into()))?;
            QwenModel::load(path)
                .ok_or_else(|| AppError::ModelLoad("Qwen3-ASR rejected its model bundle".into()))
        })
        .await
        .map_err(|error| AppError::ModelLoad(error.to_string()))??;
        *guard = Some(QwenAsrCache {
            key,
            model: Arc::clone(&model),
        });
        Ok(model)
    }

    #[cfg(target_os = "macos")]
    pub(crate) async fn clear_qwen_asr_model(&self) {
        *self.qwen_asr_model.lock().await = None;
        *self.qwen_gguf_asr_model.lock().await = None;
    }

    #[cfg(target_os = "macos")]
    pub(crate) async fn qwen_gguf_asr_model(
        &self,
        app_support_dir: PathBuf,
        spec: &'static crate::asr::AsrModelSpec,
    ) -> Result<Arc<crate::qwen_gguf_asr::QwenGgufAsrModel>> {
        let entry = crate::models::CATALOG
            .iter()
            .find(|entry| entry.id == spec.model_id)
            .ok_or_else(|| {
                AppError::ModelLoad(format!("missing catalog entry {}", spec.model_id))
            })?;
        if !entry
            .assets
            .iter()
            .all(|asset| crate::models::catalog::is_asset_downloaded(&app_support_dir, asset))
        {
            return Err(AppError::ModelNotFound(format!(
                "complete verified {} bundle; download it in Settings → AI Models",
                spec.model_id
            )));
        }
        let key = format!(
            "{}:{}:{}",
            spec.engine.as_str(),
            spec.model_id,
            spec.asset_fingerprint
        );
        let mut guard = self.qwen_gguf_asr_model.lock().await;
        if let Some(cache) = guard.as_ref().filter(|cache| cache.key == key) {
            return Ok(Arc::clone(&cache.model));
        }
        let paths = crate::models::asset_paths(&app_support_dir, spec.model_id)
            .ok_or_else(|| AppError::ModelLoad("Qwen3-ASR GGUF bundle is invalid".into()))?;
        let [model_path, mmproj_path] = paths.as_slice() else {
            return Err(AppError::ModelLoad(
                "Qwen3-ASR GGUF requires a model and audio projector".into(),
            ));
        };
        let model_path = model_path.clone();
        let mmproj_path = mmproj_path.clone();
        let backend = self.llm_runtime.shared_backend();
        let model = tokio::task::spawn_blocking(move || {
            crate::qwen_gguf_asr::QwenGgufAsrModel::load(&model_path, &mmproj_path, backend)
        })
        .await
        .map_err(|error| AppError::ModelLoad(error.to_string()))??;
        let model = Arc::new(model);
        *guard = Some(QwenGgufAsrCache {
            key,
            model: Arc::clone(&model),
        });
        Ok(model)
    }
}
