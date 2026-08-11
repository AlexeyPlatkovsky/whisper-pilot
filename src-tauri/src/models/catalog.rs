//! The static model catalog and its path/download-state resolution: the
//! trusted list of models and assets, on-disk path construction, per-target
//! asset resolution, listing, and deletion.

use crate::error::{AppError, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// One downloadable file belonging to a catalog entry. `variant_id` is `None`
/// for an asset shared by every selectable choice in its entry (e.g. the
/// diarization segmentation model) and `Some(id)` for one user-selectable
/// choice among several sharing the same entry (e.g. a diarization embedding
/// model) — `variant_label`/`recommended` are meaningful only when
/// `variant_id` is `Some`.
#[derive(Debug, PartialEq)]
pub struct ModelAsset {
    pub url: &'static str,
    pub sha256: &'static str,
    pub size_bytes: u64,
    pub file_name: &'static str,
    pub variant_id: Option<&'static str>,
    pub variant_label: Option<&'static str>,
    pub recommended: bool,
}

/// One row in the AI models section: a task and the asset(s) it needs.
pub struct ModelCatalogEntry {
    pub id: &'static str,
    pub task: &'static str,
    pub label: &'static str,
    pub assets: &'static [ModelAsset],
}

/// Beta catalog. Every URL/SHA-256/size below was verified directly against
/// the publishing source (Hugging Face CDN headers for the Whisper model;
/// GitHub release checksum.txt / a direct hash of the downloaded archive for
/// the sherpa-onnx diarization assets) — not guessed.
pub const CATALOG: &[ModelCatalogEntry] = &[
    ModelCatalogEntry {
        id: "transcription",
        task: "transcription",
        label: "Whisper large-v3-turbo (Q8)",
        assets: &[ModelAsset {
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q8_0.bin",
            sha256: "317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1",
            size_bytes: 874_188_075,
            file_name: "ggml-large-v3-turbo-q8_0.bin",
            variant_id: None,
            variant_label: None,
            recommended: false,
        }],
    },
    ModelCatalogEntry {
        id: "diarization",
        task: "diarization",
        label: "Speaker diarization (pyannote segmentation + selectable embedding)",
        assets: &[
            ModelAsset {
                url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2",
                sha256: "24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488",
                size_bytes: 6_958_444,
                file_name: "sherpa-onnx-pyannote-segmentation-3-0.tar.bz2",
                variant_id: None,
                variant_label: None,
                recommended: false,
            },
            // Independently verified 3 ways (GitHub release API asset
            // metadata, the release's checksum.txt, and a direct
            // download+hash) during the now-superseded WP-51 experiment.
            ModelAsset {
                url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx",
                sha256: "357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b",
                size_bytes: 29_596_978,
                file_name: "3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx",
                variant_id: Some("campplus"),
                variant_label: Some("CAM++"),
                recommended: false,
            },
            ModelAsset {
                url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/nemo_en_titanet_large.onnx",
                sha256: "d51abcf31717ef28162f26acb9d44dd4127c3d44c9b8624f699f3425daca8e77",
                size_bytes: 101_405_493,
                file_name: "nemo_en_titanet_large.onnx",
                variant_id: Some("titanet-large"),
                variant_label: Some("TitaNet-large"),
                recommended: true,
            },
        ],
    },
    ModelCatalogEntry {
        id: "qwen2.5-3b-q3km",
        task: "llm",
        label: "Qwen2.5 3B Instruct (Q3_K_M)",
        assets: &[ModelAsset {
            url: "https://huggingface.co/bartowski/Qwen2.5-3B-Instruct-GGUF/resolve/main/Qwen2.5-3B-Instruct-Q3_K_M.gguf",
            sha256: "8eff4e0eb51a8148abdaa9849f14f187e5ac6cd7d795610caf996a287277c59d",
            size_bytes: 1_590_475_936,
            file_name: "Qwen2.5-3B-Instruct-Q3_K_M.gguf",
            variant_id: None,
            variant_label: None,
            recommended: false,
        }],
    },
    ModelCatalogEntry {
        id: "qwen3-4b-q3kl",
        task: "llm",
        label: "Qwen3 4B (Q3_K_L)",
        assets: &[ModelAsset {
            url: "https://huggingface.co/lmstudio-community/Qwen3-4B-GGUF/resolve/main/Qwen3-4B-Q3_K_L.gguf",
            sha256: "90d5ab273b85a69e5b1cdd03dfcda82c1295fec63b3a00a35228e403b9388d1c",
            size_bytes: 2_239_785_664,
            file_name: "Qwen3-4B-Q3_K_L.gguf",
            variant_id: None,
            variant_label: None,
            recommended: true,
        }],
    },
];

/// Per-task view returned to the frontend. For an entry with selectable
/// variants (diarization), one `TaskModel` row is emitted per variant rather
/// than one row for the whole entry.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskModel {
    pub id: String,
    pub task: String,
    pub label: String,
    pub downloaded: bool,
    pub size_bytes: u64,
    pub recommended: bool,
}

pub(crate) fn models_dir(app_support_dir: &Path) -> PathBuf {
    app_support_dir.join("models")
}

pub(crate) fn asset_path(app_support_dir: &Path, asset: &ModelAsset) -> PathBuf {
    models_dir(app_support_dir).join(asset.file_name)
}

pub(crate) fn is_asset_downloaded(app_support_dir: &Path, asset: &ModelAsset) -> bool {
    std::fs::metadata(asset_path(app_support_dir, asset))
        .map(|m| m.len() == asset.size_bytes)
        .unwrap_or(false)
}

pub(crate) fn entry_downloaded(app_support_dir: &Path, entry: &ModelCatalogEntry) -> bool {
    entry
        .assets
        .iter()
        .all(|a| is_asset_downloaded(app_support_dir, a))
}

/// The assets to operate on for one download/delete-addressable id: either a
/// whole catalog entry with no selectable variants (e.g. "transcription"), or
/// one variant within an entry that has several (e.g.
/// "diarization-titanet-large").
pub enum ResolvedTarget<'a> {
    Entry(&'a ModelCatalogEntry),
    Variant {
        shared: Vec<&'a ModelAsset>,
        own: &'a ModelAsset,
    },
}

impl<'a> ResolvedTarget<'a> {
    /// Assets that must be present for this target to count as downloaded —
    /// for a variant, the shared assets plus its own asset.
    pub fn download_assets(&self) -> Vec<&'a ModelAsset> {
        match self {
            ResolvedTarget::Entry(entry) => entry.assets.iter().collect(),
            ResolvedTarget::Variant { shared, own } => {
                let mut assets = shared.clone();
                assets.push(own);
                assets
            }
        }
    }

    /// Assets to remove on delete — for a variant, only its own asset, never
    /// a shared asset another variant may still need.
    pub fn delete_assets(&self) -> Vec<&'a ModelAsset> {
        match self {
            ResolvedTarget::Entry(entry) => entry.assets.iter().collect(),
            ResolvedTarget::Variant { own, .. } => vec![own],
        }
    }

    /// The variant id this target addresses, if it addresses one selectable
    /// variant within a multi-variant entry rather than a whole entry.
    pub fn variant_id(&self) -> Option<&'a str> {
        match self {
            ResolvedTarget::Entry(_) => None,
            ResolvedTarget::Variant { own, .. } => own.variant_id,
        }
    }
}

/// Whether deleting download/delete-addressable id `deleted_id` should also
/// revert the active diarization selection to "none" — true exactly when
/// `deleted_id` names the variant `active` currently selects, so a later
/// transcription does not fail open against a model no longer on disk.
pub fn delete_clears_active_diarization_variant(deleted_id: &str, active: &str) -> bool {
    resolve_catalog_target(deleted_id)
        .and_then(|target| target.variant_id())
        .is_some_and(|variant_id| variant_id == active)
}

/// Resolve a download/delete-addressable id to its target assets. A bare
/// catalog entry id (e.g. "transcription") matches the whole entry; a
/// synthetic `"{entry_id}-{variant_id}"` id (e.g. "diarization-campplus")
/// matches one variant within an entry that has several.
pub fn resolve_catalog_target(id: &str) -> Option<ResolvedTarget<'_>> {
    if let Some(entry) = CATALOG.iter().find(|e| e.id == id) {
        return Some(ResolvedTarget::Entry(entry));
    }
    for entry in CATALOG {
        for asset in entry.assets {
            if let Some(variant_id) = asset.variant_id {
                if id == format!("{}-{variant_id}", entry.id) {
                    let shared = entry
                        .assets
                        .iter()
                        .filter(|a| a.variant_id.is_none())
                        .collect();
                    return Some(ResolvedTarget::Variant { shared, own: asset });
                }
            }
        }
    }
    None
}

/// List every catalog entry with its current downloaded state. An entry with
/// selectable variants (diarization) yields one row per variant instead of
/// one row for the whole entry.
pub fn list_task_models(app_support_dir: &Path) -> Vec<TaskModel> {
    CATALOG
        .iter()
        .flat_map(|entry| {
            let variants: Vec<&ModelAsset> = entry
                .assets
                .iter()
                .filter(|a| a.variant_id.is_some())
                .collect();
            if variants.is_empty() {
                vec![TaskModel {
                    id: entry.id.to_string(),
                    task: entry.task.to_string(),
                    label: entry.label.to_string(),
                    downloaded: entry_downloaded(app_support_dir, entry),
                    size_bytes: entry.assets.iter().map(|a| a.size_bytes).sum(),
                    recommended: false,
                }]
            } else {
                let shared: Vec<&ModelAsset> = entry
                    .assets
                    .iter()
                    .filter(|a| a.variant_id.is_none())
                    .collect();
                let shared_downloaded = shared
                    .iter()
                    .all(|a| is_asset_downloaded(app_support_dir, a));
                let shared_size: u64 = shared.iter().map(|a| a.size_bytes).sum();
                variants
                    .into_iter()
                    .map(|asset| TaskModel {
                        id: format!("{}-{}", entry.id, asset.variant_id.unwrap()),
                        task: entry.task.to_string(),
                        label: asset.variant_label.unwrap_or(entry.label).to_string(),
                        downloaded: shared_downloaded
                            && is_asset_downloaded(app_support_dir, asset),
                        size_bytes: shared_size + asset.size_bytes,
                        recommended: asset.recommended,
                    })
                    .collect()
            }
        })
        .collect()
}

/// Delete the file(s) targeted by download/delete-addressable id `id`.
/// Idempotent: a file that is already missing is not an error. An unknown id
/// is. For a variant id, only that variant's own asset is removed — the
/// shared segmentation asset and any sibling variant's asset are untouched.
pub fn delete_model(app_support_dir: &Path, id: &str) -> Result<()> {
    let target =
        resolve_catalog_target(id).ok_or_else(|| AppError::ModelCatalogNotFound(id.to_string()))?;

    for asset in target.delete_assets() {
        match std::fs::remove_file(asset_path(app_support_dir, asset)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(AppError::from(e)),
        }
    }
    Ok(())
}

/// Path to the first (in beta, only) asset of catalog entry `id`, if the id
/// is known. Used by transcribe.rs to load the model WP-39 downloaded.
pub fn primary_asset_path(app_support_dir: &Path, id: &str) -> Option<PathBuf> {
    let entry = CATALOG.iter().find(|e| e.id == id)?;
    entry.assets.first().map(|a| asset_path(app_support_dir, a))
}

/// Whether a specific diarization embedding variant has both the shared
/// segmentation model and its own embedding on disk, so a fallback retry
/// can run it without failing on a missing asset.
pub fn is_diarization_variant_downloaded(app_support_dir: &Path, variant_id: &str) -> bool {
    let Some(entry) = CATALOG.iter().find(|e| e.id == "diarization") else {
        return false;
    };
    let shared_ok = entry
        .assets
        .iter()
        .filter(|a| a.variant_id.is_none())
        .all(|a| is_asset_downloaded(app_support_dir, a));
    let variant_ok = entry
        .assets
        .iter()
        .any(|a| a.variant_id == Some(variant_id) && is_asset_downloaded(app_support_dir, a));
    shared_ok && variant_ok
}

/// Raw downloaded asset paths for catalog entry `id`, in catalog order.
/// Used by diarize.rs to locate the diarization entry's two assets (the
/// segmentation archive and the embedding model) without duplicating
/// `models_dir`/`asset_path`'s path construction.
pub fn asset_paths(app_support_dir: &Path, id: &str) -> Option<Vec<PathBuf>> {
    let entry = CATALOG.iter().find(|e| e.id == id)?;
    Some(
        entry
            .assets
            .iter()
            .map(|a| asset_path(app_support_dir, a))
            .collect(),
    )
}
