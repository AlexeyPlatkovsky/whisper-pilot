fn main() {
    // The host may not be the target; ask cargo about the target.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        add_rpaths();
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
