//! Typed microphone authorization without implicit capture or permission UI.
//!
//! Querying is side-effect free. Only the explicit request path may ask macOS
//! to show its TCC prompt, and only while the status is not yet determined.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MicrophonePermissionStatus {
    NotDetermined,
    Denied,
    Restricted,
    Authorized,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicrophonePermissionIntent {
    Query,
    Request,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicrophonePermissionDecision {
    Return(MicrophonePermissionStatus),
    RequestSystemPrompt,
}

pub fn microphone_permission_status_from_raw(raw_status: i64) -> MicrophonePermissionStatus {
    match raw_status {
        0 => MicrophonePermissionStatus::NotDetermined,
        1 => MicrophonePermissionStatus::Restricted,
        2 => MicrophonePermissionStatus::Denied,
        3 => MicrophonePermissionStatus::Authorized,
        _ => MicrophonePermissionStatus::Unavailable,
    }
}

pub fn microphone_permission_status_from_request(granted: bool) -> MicrophonePermissionStatus {
    if granted {
        MicrophonePermissionStatus::Authorized
    } else {
        MicrophonePermissionStatus::Denied
    }
}

pub fn decide_microphone_permission(
    platform_available: bool,
    current_status: MicrophonePermissionStatus,
    intent: MicrophonePermissionIntent,
) -> MicrophonePermissionDecision {
    if !platform_available {
        return MicrophonePermissionDecision::Return(MicrophonePermissionStatus::Unavailable);
    }
    match (intent, current_status) {
        (MicrophonePermissionIntent::Request, MicrophonePermissionStatus::NotDetermined) => {
            MicrophonePermissionDecision::RequestSystemPrompt
        }
        _ => MicrophonePermissionDecision::Return(current_status),
    }
}

#[cfg(target_os = "macos")]
fn system_microphone_permission_status() -> MicrophonePermissionStatus {
    use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};

    // SAFETY: AVMediaTypeAudio is a framework constant valid for the process
    // lifetime, and authorizationStatusForMediaType is a read-only class call.
    let raw_status = unsafe {
        let Some(media_type) = AVMediaTypeAudio else {
            return MicrophonePermissionStatus::Unavailable;
        };
        AVCaptureDevice::authorizationStatusForMediaType(media_type).0 as i64
    };
    microphone_permission_status_from_raw(raw_status)
}

#[cfg(not(target_os = "macos"))]
fn system_microphone_permission_status() -> MicrophonePermissionStatus {
    MicrophonePermissionStatus::Unavailable
}

/// Returns the current status without opening a device or displaying a prompt.
pub fn get_microphone_permission_status() -> MicrophonePermissionStatus {
    system_microphone_permission_status()
}

#[cfg(target_os = "macos")]
fn begin_system_microphone_permission_request() -> Option<tokio::sync::oneshot::Receiver<bool>> {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};
    use std::sync::Mutex;
    use tokio::sync::oneshot;

    let media_type = (unsafe { AVMediaTypeAudio })?;
    let (result_tx, result_rx) = oneshot::channel();
    let result_tx = Mutex::new(Some(result_tx));
    let handler = RcBlock::new(move |granted: Bool| {
        if let Ok(mut sender) = result_tx.lock() {
            if let Some(sender) = sender.take() {
                let _ = sender.send(granted.as_bool());
            }
        }
    });

    // SAFETY: `media_type` is AVMediaTypeAudio as required by AVFoundation.
    // RcBlock owns a 'static closure; AVFoundation copies it for the escaping
    // asynchronous completion and may invoke it on an arbitrary queue.
    unsafe {
        AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type, &handler);
    }
    // AVFoundation copied the escaping block during the call above. Keeping
    // the non-Send Rust handle out of the await state also lets Tauri execute
    // this command on its normal Send future path.
    drop(handler);
    Some(result_rx)
}

#[cfg(target_os = "macos")]
async fn request_system_microphone_permission() -> MicrophonePermissionStatus {
    let Some(result_rx) = begin_system_microphone_permission_request() else {
        return MicrophonePermissionStatus::Unavailable;
    };
    result_rx
        .await
        .map(microphone_permission_status_from_request)
        .unwrap_or(MicrophonePermissionStatus::Unavailable)
}

#[cfg(not(target_os = "macos"))]
async fn request_system_microphone_permission() -> MicrophonePermissionStatus {
    MicrophonePermissionStatus::Unavailable
}

/// Requests access only when the current status is `not_determined`.
/// Callers must bind this function to an explicit Recorder user action.
pub async fn request_microphone_permission() -> MicrophonePermissionStatus {
    let current_status = system_microphone_permission_status();
    match decide_microphone_permission(
        cfg!(target_os = "macos"),
        current_status,
        MicrophonePermissionIntent::Request,
    ) {
        MicrophonePermissionDecision::Return(status) => status,
        MicrophonePermissionDecision::RequestSystemPrompt => {
            request_system_microphone_permission().await
        }
    }
}
