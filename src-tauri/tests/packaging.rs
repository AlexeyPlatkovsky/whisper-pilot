//! macOS native-dylib packaging invariants (WP-60/WP-86).
//!
//! These default tests need no model, media, network, or prior Tauri bundle.
//! See `docs/architecture.md` §Build Notes for staging and rpath rationale.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The cargo profile directory (`target/debug`, `target/release`, …) — the
/// parent of the `deps/` directory this test binary was built into, which is
/// also where `sherpa-rs-sys` copies the native dylibs.
fn profile_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    exe.parent()
        .and_then(Path::parent)
        .expect("profile dir above deps/")
        .to_path_buf()
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The executable that gets bundled into `Contents/MacOS`. Every assertion here
/// is about that artifact, not about the test harness — `rustc-link-arg` covers
/// test binaries too, so checking the running executable would pass without
/// proving anything about what ships.
fn app_binary() -> PathBuf {
    let binary = profile_dir().join("whisper-pilot");
    assert!(
        binary.is_file(),
        "{} not found — build the app binary before running this test",
        binary.display()
    );
    binary
}

/// The `bundle.macOS.frameworks` entries declared in `tauri.conf.json`.
fn declared_frameworks() -> Vec<String> {
    let path = manifest_dir().join("tauri.conf.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let conf: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));
    conf["bundle"]["macOS"]["frameworks"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|e| {
                    e.as_str()
                        .expect("bundle.macOS.frameworks entries are strings")
                        .to_string()
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn bundle_stages_native_dylibs_after_cargo_build() {
    let path = manifest_dir().join("tauri.conf.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let conf: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));
    assert_eq!(
        conf["build"]["beforeBundleCommand"], "sh scripts/stage-native-dylibs.sh",
        "native dylibs must be staged after Cargo produces them and before Tauri bundles"
    );
}

/// The dylibs the shipped executable loads through `@rpath`, by file name.
///
/// Taken from the binary rather than from whatever `lib*.dylib` files sit in
/// the profile directory: that directory also holds this crate's own `cdylib`
/// output and sherpa's unused C++ API, neither of which belongs in the bundle.
fn rpath_dependencies_of_app_binary() -> Vec<String> {
    let binary = app_binary();
    let out = Command::new("otool")
        .arg("-L")
        .arg(&binary)
        .output()
        .unwrap_or_else(|e| panic!("running otool on {}: {e}", binary.display()));
    assert!(
        out.status.success(),
        "otool -L {} failed: {}",
        binary.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let mut names: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.trim().strip_prefix("@rpath/"))
        .filter_map(|rest| rest.split_whitespace().next())
        .map(str::to_string)
        .collect();
    names.sort();
    names.dedup();
    names
}

fn rpaths_of(binary: &Path) -> Vec<String> {
    let out = Command::new("otool")
        .arg("-l")
        .arg(binary)
        .output()
        .unwrap_or_else(|e| panic!("running otool on {}: {e}", binary.display()));
    assert!(
        out.status.success(),
        "otool -l {} failed: {}",
        binary.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let mut rpaths = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.trim() != "cmd LC_RPATH" {
            continue;
        }
        // cmd LC_RPATH / cmdsize <n> / path <value> (offset <n>)
        for follow in lines.by_ref().take(3) {
            if let Some(rest) = follow.trim().strip_prefix("path ") {
                rpaths.push(
                    rest.split(" (offset")
                        .next()
                        .unwrap_or(rest)
                        .trim()
                        .to_string(),
                );
                break;
            }
        }
    }
    rpaths
}

/// `@rpath` dylibs this app never bundles because they're OS-provided,
/// resolved instead via a fixed system `LC_RPATH` (see `add_rpaths` in
/// `build.rs`) rather than `Contents/Frameworks`. Unlike sherpa-onnx/
/// onnxruntime, these have shipped as part of the OS since Swift ABI
/// stability (macOS 10.14.4+, well below this app's deployment target) and
/// exist only in the dyld shared cache — there is no standalone file on disk
/// to bundle even if we wanted to.
const OS_PROVIDED_DYLIBS: [&str; 1] = ["libswift_Concurrency.dylib"];

/// The system rpath OS-provided dylibs above resolve against. Asserted
/// separately below so this exemption can't silently mask a missing rpath.
const OS_PROVIDED_DYLIB_RPATH: &str = "/usr/lib/swift";

/// The bundler only copies what the config names, so an `@rpath` dependency
/// the config omits — and that isn't `OS_PROVIDED_DYLIBS` — is one that will
/// be missing from the `.app`. This is what catches a sherpa-onnx upgrade
/// that renames or adds a dylib.
#[test]
fn bundle_config_declares_every_native_dylib() {
    let required = rpath_dependencies_of_app_binary();
    assert!(
        !required.is_empty(),
        "the app binary loads no @rpath dylibs — this test is asserting nothing"
    );

    let declared = declared_frameworks();
    let declared_names: Vec<&str> = declared
        .iter()
        .filter_map(|p| Path::new(p).file_name().and_then(|n| n.to_str()))
        .collect();

    let missing: Vec<&String> = required
        .iter()
        .filter(|name| {
            !declared_names.contains(&name.as_str()) && !OS_PROVIDED_DYLIBS.contains(&name.as_str())
        })
        .collect();
    assert!(
        missing.is_empty(),
        "tauri.conf.json bundle.macOS.frameworks does not declare {missing:?}; \
         declared: {declared:?}. Those dylibs would be absent from Contents/Frameworks."
    );
}

/// The exemption above is only sound if the binary can actually resolve an
/// OS-provided dylib without bundling it — i.e. it carries the fixed system
/// rpath those dylibs load against. Catches `add_rpaths` losing that rpath
/// while an OS-provided dylib is still linked in.
#[test]
fn os_provided_dylibs_have_their_system_rpath() {
    let required = rpath_dependencies_of_app_binary();
    if !required
        .iter()
        .any(|name| OS_PROVIDED_DYLIBS.contains(&name.as_str()))
    {
        return;
    }
    let rpaths = rpaths_of(&app_binary());
    assert!(
        rpaths.iter().any(|p| p == OS_PROVIDED_DYLIB_RPATH),
        "the app binary loads an OS-provided dylib ({OS_PROVIDED_DYLIBS:?}) but carries no \
         {OS_PROVIDED_DYLIB_RPATH} rpath (found {rpaths:?}); it would abort at dyld"
    );
}

/// Every declared framework must have a source dylib in Cargo's profile
/// directory. The ignored `src-tauri/frameworks/` directory is generated only
/// by `beforeBundleCommand`, so a default test must not require a prior bundle.
#[test]
fn declared_bundle_framework_sources_exist_in_profile() {
    let declared = declared_frameworks();
    assert!(
        !declared.is_empty(),
        "tauri.conf.json declares no bundle.macOS.frameworks"
    );

    for entry in &declared {
        let name = Path::new(entry)
            .file_name()
            .expect("framework entry has a file name");
        let path = profile_dir().join(name);
        assert!(
            path.is_file(),
            "declared framework {entry} has no post-Cargo source at {}",
            path.display()
        );
    }
}

/// `@executable_path/../Frameworks` is what makes `Contents/MacOS/whisper-pilot`
/// resolve `@rpath/…` against `Contents/Frameworks` once installed, with no
/// help from the launcher's environment.
#[test]
fn app_binary_carries_bundle_relative_rpath() {
    let exe = app_binary();
    let rpaths = rpaths_of(&exe);
    assert!(
        rpaths.iter().any(|p| p == "@executable_path/../Frameworks"),
        "{} carries no @executable_path/../Frameworks rpath (found {rpaths:?}); \
         a bundled build would abort at dyld with \"no LC_RPATH's found\"",
        exe.display()
    );
}

/// Outside a bundle the binary sits next to its dylibs, so the same link-time
/// rpath set has to cover the plain-executable layout too — otherwise the
/// build only runs under cargo, which exports a dyld fallback path for it.
#[test]
fn app_binary_carries_sibling_rpath() {
    let exe = app_binary();
    let rpaths = rpaths_of(&exe);
    assert!(
        rpaths.iter().any(|p| p == "@executable_path"),
        "{} carries no @executable_path rpath (found {rpaths:?})",
        exe.display()
    );
}
