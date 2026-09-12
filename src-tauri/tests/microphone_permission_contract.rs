use whisperpilot_lib::microphone_permission::{
    decide_microphone_permission, microphone_permission_status_from_raw,
    microphone_permission_status_from_request, MicrophonePermissionDecision,
    MicrophonePermissionIntent, MicrophonePermissionStatus,
};

const LIB_RS: &str = include_str!("../src/lib.rs");
const IPC_TS: &str = include_str!("../../src/ipc.ts");

fn exported_function_body<'a>(source: &'a str, name: &str) -> &'a str {
    let marker = format!("export function {name}(");
    let (_, after_marker) = source
        .split_once(&marker)
        .unwrap_or_else(|| panic!("missing exported function {name}"));
    let (_, body) = after_marker
        .split_once('{')
        .unwrap_or_else(|| panic!("missing body for exported function {name}"));
    body.split_once("\n}")
        .unwrap_or_else(|| panic!("unterminated exported function {name}"))
        .0
}

#[test]
fn raw_av_authorization_status_maps_without_opening_tcc() {
    assert_eq!(
        microphone_permission_status_from_raw(0),
        MicrophonePermissionStatus::NotDetermined
    );
    assert_eq!(
        microphone_permission_status_from_raw(1),
        MicrophonePermissionStatus::Restricted
    );
    assert_eq!(
        microphone_permission_status_from_raw(2),
        MicrophonePermissionStatus::Denied
    );
    assert_eq!(
        microphone_permission_status_from_raw(3),
        MicrophonePermissionStatus::Authorized
    );
    assert_eq!(
        microphone_permission_status_from_raw(99),
        MicrophonePermissionStatus::Unavailable
    );
}

#[test]
fn permission_status_serializes_as_snake_case() {
    let cases = [
        (MicrophonePermissionStatus::NotDetermined, "not_determined"),
        (MicrophonePermissionStatus::Denied, "denied"),
        (MicrophonePermissionStatus::Restricted, "restricted"),
        (MicrophonePermissionStatus::Authorized, "authorized"),
        (MicrophonePermissionStatus::Unavailable, "unavailable"),
    ];

    for (status, expected) in cases {
        assert_eq!(
            serde_json::to_string(&status).unwrap(),
            format!("\"{expected}\"")
        );
    }
}

#[test]
fn query_never_requests_the_system_prompt() {
    for status in [
        MicrophonePermissionStatus::NotDetermined,
        MicrophonePermissionStatus::Denied,
        MicrophonePermissionStatus::Restricted,
        MicrophonePermissionStatus::Authorized,
        MicrophonePermissionStatus::Unavailable,
    ] {
        assert_eq!(
            decide_microphone_permission(true, status, MicrophonePermissionIntent::Query),
            MicrophonePermissionDecision::Return(status)
        );
    }
}

#[test]
fn request_only_prompts_when_not_determined_and_maps_completion() {
    assert_eq!(
        decide_microphone_permission(
            true,
            MicrophonePermissionStatus::NotDetermined,
            MicrophonePermissionIntent::Request,
        ),
        MicrophonePermissionDecision::RequestSystemPrompt
    );
    assert_eq!(
        decide_microphone_permission(
            true,
            MicrophonePermissionStatus::Denied,
            MicrophonePermissionIntent::Request,
        ),
        MicrophonePermissionDecision::Return(MicrophonePermissionStatus::Denied)
    );
    assert_eq!(
        microphone_permission_status_from_request(true),
        MicrophonePermissionStatus::Authorized
    );
    assert_eq!(
        microphone_permission_status_from_request(false),
        MicrophonePermissionStatus::Denied
    );
}

#[test]
fn unavailable_platform_never_prompts() {
    for intent in [
        MicrophonePermissionIntent::Query,
        MicrophonePermissionIntent::Request,
    ] {
        assert_eq!(
            decide_microphone_permission(false, MicrophonePermissionStatus::NotDetermined, intent,),
            MicrophonePermissionDecision::Return(MicrophonePermissionStatus::Unavailable)
        );
    }
}

#[test]
fn tauri_registers_the_two_approved_recorder_permission_commands() {
    let (_, after_marker) = LIB_RS
        .split_once("tauri::generate_handler![")
        .expect("lib.rs must register a Tauri handler");
    let handler = after_marker
        .split_once("])")
        .expect("Tauri handler registration must terminate")
        .0;
    let recorder_commands: Vec<_> = handler
        .split(',')
        .map(str::trim)
        .filter(|command| command.starts_with("commands::recorder::"))
        .collect();

    assert!(recorder_commands.contains(&"commands::recorder::get_microphone_permission_status"));
    assert!(recorder_commands.contains(&"commands::recorder::request_microphone_permission"));
}

#[test]
fn typescript_exports_the_exact_microphone_permission_union() {
    let (_, after_marker) = IPC_TS
        .split_once("export type MicrophonePermissionStatus =")
        .expect("ipc.ts must export MicrophonePermissionStatus");
    let declaration = after_marker
        .split_once(';')
        .expect("MicrophonePermissionStatus must terminate with a semicolon")
        .0;
    let mut actual: Vec<_> = declaration
        .split('|')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect();
    let mut expected = vec![
        "\"not_determined\"",
        "\"denied\"",
        "\"restricted\"",
        "\"authorized\"",
        "\"unavailable\"",
    ];
    actual.sort_unstable();
    expected.sort_unstable();

    assert_eq!(actual, expected);
}

#[test]
fn typescript_permission_wrappers_invoke_the_approved_commands() {
    for (wrapper, command) in [
        (
            "getMicrophonePermissionStatus",
            "get_microphone_permission_status",
        ),
        (
            "requestMicrophonePermission",
            "request_microphone_permission",
        ),
    ] {
        let body = exported_function_body(IPC_TS, wrapper);
        let invocation = format!("invoke<MicrophonePermissionStatus>(\"{command}\")");
        assert_eq!(
            body.matches(&invocation).count(),
            1,
            "{wrapper} must invoke exactly {command}"
        );
    }
}
