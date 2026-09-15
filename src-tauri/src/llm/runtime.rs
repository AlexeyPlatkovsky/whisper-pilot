use super::*;

use crate::error::{AppError, Result};
use crate::models::catalog::{llm_spec_by_file_name, LlmModelSpec};
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub(crate) const MAX_NEW_TOKENS: i32 = 1024;
const TRANSLATION_MAX_NEW_TOKENS: i32 = 256;
const TRANSLATION_PREVIEW_MAX_NEW_TOKENS: i32 = 128;
pub(crate) const CTX_SIZE: u32 = 16384;
pub(crate) const N_BATCH: u32 = 2048;
pub(crate) const LLM_MODEL_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFingerprint {
    model_id: String,
    asset_hash: String,
}

impl ModelFingerprint {
    pub fn new(model_id: impl Into<String>, asset_hash: impl Into<String>) -> Self {
        Self {
            model_id: model_id.into(),
            asset_hash: asset_hash.into(),
        }
    }
}

pub struct SelectedModelCache<T> {
    pub(crate) selected: Option<(ModelFingerprint, Arc<T>)>,
}

impl<T> Default for SelectedModelCache<T> {
    fn default() -> Self {
        Self { selected: None }
    }
}

impl<T> SelectedModelCache<T> {
    pub fn get_or_try_load(
        &mut self,
        fingerprint: &ModelFingerprint,
        load: impl FnOnce() -> Result<T>,
    ) -> Result<Arc<T>> {
        if let Some((cached_fingerprint, model)) = &self.selected {
            if cached_fingerprint == fingerprint {
                return Ok(Arc::clone(model));
            }
        }
        // Loading a different llama.cpp model requires the previous backend
        // to be fully released first. Scheduler serialization guarantees no
        // active job still depends on the cache-owned Arc at this boundary.
        self.selected = None;
        let model = Arc::new(load()?);
        self.selected = Some((fingerprint.clone(), Arc::clone(&model)));
        Ok(model)
    }

    pub fn clear(&mut self) {
        self.selected = None;
    }
}

const LLM_QUEUE_CAPACITY: usize = 8;

pub(crate) struct SharedLlamaBackend {
    backend: OnceLock<LlamaBackend>,
    init: Mutex<()>,
}

impl Default for SharedLlamaBackend {
    fn default() -> Self {
        Self {
            backend: OnceLock::new(),
            init: Mutex::new(()),
        }
    }
}

impl SharedLlamaBackend {
    pub(crate) fn get(&self) -> Result<&LlamaBackend> {
        if let Some(backend) = self.backend.get() {
            return Ok(backend);
        }
        let _init = self
            .init
            .lock()
            .map_err(|_| AppError::Llm("llama.cpp backend init lock is poisoned".into()))?;
        if self.backend.get().is_none() {
            let backend = LlamaBackend::init().map_err(|error| AppError::Llm(error.to_string()))?;
            let _ = self.backend.set(backend);
        }
        self.backend
            .get()
            .ok_or_else(|| AppError::Llm("llama.cpp backend did not initialize".into()))
    }
}

pub(crate) struct LoadedLlmModel {
    // Field order is intentional: the model must be freed before its backend.
    pub(crate) model: LlamaModel,
    pub(crate) backend: Arc<SharedLlamaBackend>,
    pub(crate) spec: Option<&'static LlmModelSpec>,
}

pub fn run_with_token_preflight<T>(
    prompt_tokens: usize,
    reserved_output_tokens: usize,
    context_size: usize,
    decode: impl FnOnce() -> Result<T>,
) -> Result<T> {
    if prompt_tokens.saturating_add(reserved_output_tokens) > context_size {
        return Err(AppError::Llm(
            "input exceeds the local model context window".to_string(),
        ));
    }
    decode()
}

impl LoadedLlmModel {
    fn load(path: &Path, backend: Arc<SharedLlamaBackend>) -> Result<Self> {
        let model = LlamaModel::load_from_file(backend.get()?, path, &LlamaModelParams::default())
            .map_err(|error| AppError::Llm(format!("model load: {error}")))?;
        let spec = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(llm_spec_by_file_name);
        Ok(Self {
            model,
            backend,
            spec,
        })
    }
}

enum IdleUnloadSignal {
    Reset,
    Shutdown,
}

pub(crate) struct IdleModelUnloader {
    sender: Sender<IdleUnloadSignal>,
    worker: Option<JoinHandle<()>>,
}

impl IdleModelUnloader {
    pub(crate) fn new<T: Send + Sync + 'static>(
        schedule: Arc<Mutex<BoundedLlmScheduler>>,
        cache: Arc<Mutex<SelectedModelCache<T>>>,
        timeout: Duration,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("llm-idle-unloader".to_string())
            .spawn(move || {
                let mut deadline: Option<Instant> = None;
                loop {
                    let signal = match deadline {
                        Some(deadline) => receiver
                            .recv_timeout(deadline.saturating_duration_since(Instant::now())),
                        None => receiver.recv().map_err(|_| RecvTimeoutError::Disconnected),
                    };
                    match signal {
                        Ok(IdleUnloadSignal::Reset) => {
                            deadline = Some(Instant::now() + timeout);
                        }
                        Ok(IdleUnloadSignal::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                            break;
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            let schedule = match schedule.lock() {
                                Ok(schedule) => schedule,
                                Err(_) => break,
                            };
                            if schedule.active.is_none() && schedule.pending.is_empty() {
                                if let Ok(mut cache) = cache.lock() {
                                    cache.clear();
                                }
                            }
                            deadline = None;
                        }
                    }
                }
            })
            .expect("spawn local LLM idle unloader");
        Self {
            sender,
            worker: Some(worker),
        }
    }

    pub(crate) fn reset(&self) {
        let _ = self.sender.send(IdleUnloadSignal::Reset);
    }
}

impl Drop for IdleModelUnloader {
    fn drop(&mut self) {
        let _ = self.sender.send(IdleUnloadSignal::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Application-owned llama.cpp runtime. Calls execute serially because one
/// shared model is substantially cheaper and safer than racing multiple Metal
/// contexts. Pending jobs are bounded and ordered by user-visible urgency.
///
/// The runtime lives in Tauri's managed [`crate::state::AppState`] rather than
/// a process static so cached Metal resources are released before llama.cpp's
/// process-global backend teardown.
pub struct LlmRuntime {
    idle_unloader: IdleModelUnloader,
    backend: Arc<SharedLlamaBackend>,
    pub(crate) schedule: Arc<Mutex<BoundedLlmScheduler>>,
    pub(crate) schedule_changed: Condvar,
    cache: Arc<Mutex<SelectedModelCache<LoadedLlmModel>>>,
    next_job_id: AtomicU64,
    model_generation: AtomicU64,
}

impl Default for LlmRuntime {
    fn default() -> Self {
        let schedule = Arc::new(Mutex::new(BoundedLlmScheduler::new(LLM_QUEUE_CAPACITY)));
        let cache = Arc::new(Mutex::new(SelectedModelCache::default()));
        Self {
            idle_unloader: IdleModelUnloader::new(
                Arc::clone(&schedule),
                Arc::clone(&cache),
                LLM_MODEL_IDLE_TIMEOUT,
            ),
            backend: Arc::new(SharedLlamaBackend::default()),
            schedule,
            schedule_changed: Condvar::new(),
            cache,
            next_job_id: AtomicU64::new(1),
            model_generation: AtomicU64::new(1),
        }
    }
}

impl LlmRuntime {
    pub(crate) fn shared_backend(&self) -> Arc<SharedLlamaBackend> {
        Arc::clone(&self.backend)
    }
    pub fn infer(&self, model_path: &Path, kind: LlmJobKind, prompt: &str) -> Result<String> {
        self.infer_with_model_resolver(kind, prompt, || Ok(model_path.to_path_buf()))
    }

    /// Resolve the selected model only after this job owns the scheduler
    /// lease. A queued model-mutation barrier therefore either waits for this
    /// active job or completes first and cancels this job's older generation;
    /// no command can resolve an obsolete path in the gap between Settings
    /// mutation and scheduler admission.
    pub fn infer_with_model_resolver(
        &self,
        kind: LlmJobKind,
        prompt: &str,
        resolve_model: impl FnOnce() -> Result<std::path::PathBuf>,
    ) -> Result<String> {
        ensure_prompt_fits_context_budget(prompt)?;
        let model_generation = self.model_generation.load(Ordering::Acquire);
        let id = self.next_job_id.fetch_add(1, Ordering::Relaxed);
        let _lease = self.acquire_execution_lease(ScheduledLlmJob::new(id, kind))?;

        if self.model_generation.load(Ordering::Acquire) == model_generation {
            resolve_model().and_then(|model_path| self.run_job(&model_path, kind, prompt))
        } else {
            Err(AppError::Llm(
                "local LLM job was cancelled because the selected model changed".to_string(),
            ))
        }
    }

    /// Run a selected-model setting or deletion mutation after active
    /// inference completes. The generation bump cancels already-queued jobs
    /// that captured the old selection before the mutation barrier.
    pub fn mutate_selected_model<T>(&self, mutate: impl FnOnce() -> Result<T>) -> Result<T> {
        let id = self.next_job_id.fetch_add(1, Ordering::Relaxed);
        let _lease =
            self.acquire_execution_lease(ScheduledLlmJob::new(id, LlmJobKind::ModelMutation))?;
        self.model_generation.fetch_add(1, Ordering::AcqRel);
        self.invalidate();
        mutate()
    }

    /// Owns one scheduler admission until it is dropped.  This is deliberately
    /// an RAII guard rather than a `catch_unwind` wrapper: callers keep their
    /// ordinary panic semantics while a resolver, loader, or inference panic
    /// cannot strand the single-flight scheduler in its active state.
    fn acquire_execution_lease(&self, job: ScheduledLlmJob) -> Result<ExecutionLease<'_>> {
        self.wait_for_execution_lease(job)?;
        Ok(ExecutionLease {
            runtime: self,
            id: job.id,
        })
    }

    fn finish_execution_lease(&self, id: u64) -> Result<()> {
        let mut schedule = self
            .schedule
            .lock()
            .map_err(|_| AppError::Llm("local LLM scheduler lock is poisoned".to_string()))?;
        schedule
            .finish(id)
            .map_err(|_| AppError::Llm("local LLM scheduler lost its active job".to_string()))?;
        let _ = schedule.start_next();
        self.schedule_changed.notify_all();
        drop(schedule);
        self.idle_unloader.reset();
        Ok(())
    }

    fn invalidate(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    fn wait_for_execution_lease(&self, job: ScheduledLlmJob) -> Result<()> {
        let mut schedule = self
            .schedule
            .lock()
            .map_err(|_| AppError::Llm("local LLM scheduler lock is poisoned".to_string()))?;
        schedule.try_enqueue(job).map_err(|error| match error {
            LlmScheduleError::QueueFull { capacity } => AppError::Llm(format!(
                "local LLM queue is full ({capacity} pending jobs); retry shortly"
            )),
            LlmScheduleError::UnknownActiveJob { .. } => {
                AppError::Llm("local LLM scheduler rejected the job".to_string())
            }
        })?;
        let _ = schedule.start_next();
        while schedule.active != Some(job) {
            schedule = self
                .schedule_changed
                .wait(schedule)
                .map_err(|_| AppError::Llm("local LLM scheduler lock is poisoned".to_string()))?;
        }
        Ok(())
    }

    fn run_job(&self, model_path: &Path, kind: LlmJobKind, prompt: &str) -> Result<String> {
        let fingerprint = model_fingerprint(model_path)?;
        let model = self
            .cache
            .lock()
            .map_err(|_| AppError::Llm("local LLM model cache lock is poisoned".to_string()))?
            .get_or_try_load(&fingerprint, || {
                LoadedLlmModel::load(model_path, self.shared_backend())
            })?;
        run_inference_with_model(&model, kind, prompt)
    }
}

struct ExecutionLease<'a> {
    runtime: &'a LlmRuntime,
    id: u64,
}

impl Drop for ExecutionLease<'_> {
    fn drop(&mut self) {
        // There is no recoverable caller at Drop time.  `finish` only fails
        // for a poisoned scheduler or an impossible ownership violation; in
        // either case do not mask an in-flight panic.  The normal path still
        // releases and wakes the next queued job, including unwinding paths.
        let _ = self.runtime.finish_execution_lease(self.id);
    }
}

pub(crate) fn model_fingerprint(model_path: &Path) -> Result<ModelFingerprint> {
    let canonical = model_path.canonicalize().map_err(|error| {
        AppError::Llm(format!(
            "could not fingerprint model at {}: {error}",
            model_path.display()
        ))
    })?;
    let metadata = canonical.metadata().map_err(|error| {
        AppError::Llm(format!(
            "could not read model metadata at {}: {error}",
            canonical.display()
        ))
    })?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());
    let catalog_hash = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|file_name| {
            crate::models::CATALOG
                .iter()
                .flat_map(|entry| entry.assets.iter())
                .find(|asset| asset.file_name == file_name)
                .map(|asset| asset.sha256)
        });
    // Catalog assets receive a full SHA-256 verification before their atomic
    // install. We retain that trusted identity *and* sample a bounded 128 KiB
    // of the installed bytes, so even a later same-size/same-mtime replacement
    // does not silently reuse a stale cache. This deliberately never hashes a
    // multi-gigabyte GGUF on the inference hot path.
    let catalog_identity = catalog_hash.unwrap_or("unmanaged");
    let content_identity = bounded_file_digest(&canonical, metadata.len())?;
    Ok(ModelFingerprint::new(
        canonical.to_string_lossy(),
        format!(
            "{}:{modified}:catalog:{catalog_identity}:sample:{content_identity}",
            metadata.len()
        ),
    ))
}

const FINGERPRINT_SAMPLE_BYTES: u64 = 64 * 1024;

fn bounded_file_digest(path: &Path, length: u64) -> Result<String> {
    let mut file = std::fs::File::open(path).map_err(|error| {
        AppError::Llm(format!(
            "could not sample model {}: {error}",
            path.display()
        ))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; FINGERPRINT_SAMPLE_BYTES.min(length) as usize];
    file.read_exact(&mut buffer).map_err(|error| {
        AppError::Llm(format!(
            "could not read model sample {}: {error}",
            path.display()
        ))
    })?;
    hasher.update(&buffer);
    if length > FINGERPRINT_SAMPLE_BYTES {
        let tail_start = length.saturating_sub(FINGERPRINT_SAMPLE_BYTES);
        file.seek(SeekFrom::Start(tail_start)).map_err(|error| {
            AppError::Llm(format!(
                "could not seek model sample {}: {error}",
                path.display()
            ))
        })?;
        let mut tail = vec![0_u8; FINGERPRINT_SAMPLE_BYTES.min(length) as usize];
        file.read_exact(&mut tail).map_err(|error| {
            AppError::Llm(format!(
                "could not read model sample {}: {error}",
                path.display()
            ))
        })?;
        hasher.update(&tail);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmJobKind {
    ModelMutation,
    Translation,
    TranslationPreview,
    Mfu,
    Prettify,
}

impl LlmJobKind {
    fn priority(self) -> u8 {
        match self {
            Self::ModelMutation => 0,
            Self::Translation => 1,
            Self::TranslationPreview => 2,
            Self::Mfu => 3,
            Self::Prettify => 4,
        }
    }

    pub(crate) fn max_new_tokens(self) -> i32 {
        match self {
            Self::Translation => TRANSLATION_MAX_NEW_TOKENS,
            Self::TranslationPreview => TRANSLATION_PREVIEW_MAX_NEW_TOKENS,
            Self::ModelMutation | Self::Mfu | Self::Prettify => MAX_NEW_TOKENS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledLlmJob {
    id: u64,
    kind: LlmJobKind,
}

impl ScheduledLlmJob {
    pub fn new(id: u64, kind: LlmJobKind) -> Self {
        Self { id, kind }
    }

    pub fn id(self) -> u64 {
        self.id
    }

    pub fn kind(self) -> LlmJobKind {
        self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmScheduleError {
    QueueFull { capacity: usize },
    UnknownActiveJob { id: u64 },
}

pub struct BoundedLlmScheduler {
    capacity: usize,
    pub(crate) pending: VecDeque<ScheduledLlmJob>,
    pub(crate) active: Option<ScheduledLlmJob>,
}

impl BoundedLlmScheduler {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            pending: VecDeque::with_capacity(capacity),
            active: None,
        }
    }

    pub fn try_enqueue(
        &mut self,
        job: ScheduledLlmJob,
    ) -> std::result::Result<(), LlmScheduleError> {
        if self.pending.len() >= self.capacity {
            return Err(LlmScheduleError::QueueFull {
                capacity: self.capacity,
            });
        }
        let position = self
            .pending
            .iter()
            .position(|queued| queued.kind.priority() > job.kind.priority())
            .unwrap_or(self.pending.len());
        self.pending.insert(position, job);
        Ok(())
    }

    pub fn start_next(&mut self) -> Option<ScheduledLlmJob> {
        if self.active.is_some() {
            return None;
        }
        let job = self.pending.pop_front()?;
        self.active = Some(job);
        Some(job)
    }

    pub fn finish(&mut self, id: u64) -> std::result::Result<(), LlmScheduleError> {
        match self.active {
            Some(active) if active.id == id => {
                self.active = None;
                Ok(())
            }
            _ => Err(LlmScheduleError::UnknownActiveJob { id }),
        }
    }
}
