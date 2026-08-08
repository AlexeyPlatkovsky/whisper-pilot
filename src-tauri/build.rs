use std::path::PathBuf;

/// Staging directory for the native dylibs, matching `bundle.macOS.frameworks`
/// in `tauri.conf.json`. Generated, not checked in.
const FRAMEWORKS_DIR: &str = "frameworks";

/// The `@rpath` dependencies of the built binary: sherpa-onnx's C API and the
/// ONNX Runtime it loads. `sherpa-rs-sys` (feature `download-binaries`) drops
/// both into the cargo profile directory while building this crate's deps.
const NATIVE_DYLIBS: [&str; 2] = ["libsherpa-onnx-c-api.dylib", "libonnxruntime.1.17.1.dylib"];

fn main() {
    // The host may not be the target; ask cargo about the target.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        add_rpaths();
        stage_native_dylibs();
    }
    tauri_build::build()
}

/// The linker records the natives as `@rpath/…`, so without an `LC_RPATH` to
/// expand that against, dyld aborts before `main` — which is what a
/// DMG-installed build did (WP-60). `../Frameworks` resolves them inside an
/// installed `.app`; `@executable_path` covers a binary run straight out of the
/// profile directory, where the dylibs are siblings.
fn add_rpaths() {
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path");
    // Streaming's system-audio capture (WP-70, screencapturekit) pulls in
    // apple-metal, which links against libswift_Concurrency.dylib. That's an
    // OS-provided Swift runtime library (present on every Mac since Swift ABI
    // stability, no bundling needed like the dylibs above) but it lives only
    // in the dyld shared cache, not the executable's own rpaths, so without
    // this the binary links but aborts at launch with "Library not loaded:
    // @rpath/libswift_Concurrency.dylib". A fixed absolute path, not
    // `@executable_path`-relative, since nothing here is bundled.
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

/// Copy the natives to a stable path the bundler can name: `tauri.conf.json`
/// cannot reference the profile directory, which moves with the profile and
/// `--target`.
fn stage_native_dylibs() {
    let profile_dir = profile_dir();
    let staging = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
        .join(FRAMEWORKS_DIR);
    std::fs::create_dir_all(&staging)
        .unwrap_or_else(|e| panic!("creating {}: {e}", staging.display()));

    for name in NATIVE_DYLIBS {
        let src = profile_dir.join(name);
        // A rename or version bump upstream must fail the build here rather
        // than produce an `.app` that dies at launch on someone else's Mac.
        assert!(
            src.is_file(),
            "{} not found — sherpa-rs-sys did not produce it. If the upstream \
             dylib set changed, update NATIVE_DYLIBS and bundle.macOS.frameworks.",
            src.display()
        );
        std::fs::copy(&src, staging.join(name))
            .unwrap_or_else(|e| panic!("staging {}: {e}", src.display()));
    }
}

/// `OUT_DIR` is `<profile>/build/<pkg>-<hash>/out`; the dylibs sit in
/// `<profile>`.
fn profile_dir() -> PathBuf {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    out_dir
        .ancestors()
        .nth(3)
        .unwrap_or_else(|| panic!("no profile dir above OUT_DIR {}", out_dir.display()))
        .to_path_buf()
}
