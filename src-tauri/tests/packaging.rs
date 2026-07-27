//! WP-60 regression: a `.app` built from this configuration must be able to
//! find its native dylibs.
//!
//! `sherpa-rs-sys` (feature `download-binaries`) drops
//! `libsherpa-onnx-c-api.dylib` and `libonnxruntime.<version>.dylib` into the
//! cargo profile directory, and the linker records them as
//! `@rpath/…`. Two things then have to be true for a packaged build to launch:
//! the dylibs have to reach `Contents/Frameworks`, and the executable has to
//! carry an `LC_RPATH` that points there. Before this work neither was true —
//! `tauri.conf.json` had no `bundle.macOS.frameworks` entry and the binary had
//! zero `LC_RPATH` load commands — so a DMG-installed build aborted at dyld
//! with `Library not loaded: @rpath/libonnxruntime.1.17.1.dylib … Reason: no
//! LC_RPATH's found`, which macOS reports as a vague "works with this version
//! of macOS" dialog.
//!
//! These run by default: they need no models, media, or network, only the
//! artifacts a build has already produced.

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

/// The dylibs the shipped executable loads through `@rpath`, by file name.
///
/// Taken from the binary rather than from whatever `lib*.dylib` files sit in
/// the profile directory: that directory also holds this crate's own `cdylib`
/// output and sherpa's unused C++ API, neither of which belongs in the bundle.
fn rpath_dependencies_of_app_binary() -> Vec<String> {
    let binary = profile_dir().join("whisper-pilot");
    assert!(
        binary.is_file(),
        "{} not found — build the app binary before running this test",
        binary.display()
    );
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

/// The bundler only copies what the config names, so an `@rpath` dependency
/// the config omits is one that will be missing from the `.app`. This is what
/// catches a sherpa-onnx upgrade that renames or adds a dylib.
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
        .filter(|name| !declared_names.contains(&name.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "tauri.conf.json bundle.macOS.frameworks does not declare {missing:?}; \
         declared: {declared:?}. Those dylibs would be absent from Contents/Frameworks."
    );
}

/// A declared framework path that does not exist when the bundler runs fails
/// the build at best and silently ships a broken `.app` at worst.
#[test]
fn declared_bundle_frameworks_exist_on_disk() {
    let declared = declared_frameworks();
    assert!(
        !declared.is_empty(),
        "tauri.conf.json declares no bundle.macOS.frameworks"
    );

    for entry in &declared {
        let path = manifest_dir().join(entry);
        assert!(
            path.is_file(),
            "declared framework {entry} resolves to {}, which does not exist",
            path.display()
        );
    }
}

/// `@executable_path/../Frameworks` is what makes `Contents/MacOS/whisper-pilot`
/// resolve `@rpath/…` against `Contents/Frameworks` once installed, with no
/// help from the launcher's environment.
#[test]
fn linked_binaries_carry_bundle_relative_rpath() {
    let exe = std::env::current_exe().expect("current_exe");
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
fn linked_binaries_carry_sibling_rpath() {
    let exe = std::env::current_exe().expect("current_exe");
    let rpaths = rpaths_of(&exe);
    assert!(
        rpaths.iter().any(|p| p == "@executable_path"),
        "{} carries no @executable_path rpath (found {rpaths:?})",
        exe.display()
    );
}
