fn main() {
    // `analytics.rs` reads this through `option_env!`, which is resolved at
    // compile time — without the declaration, a key exported after the first
    // build stays baked in as absent until something else forces a rebuild.
    println!("cargo:rerun-if-env-changed=POSTHOG_KEY");
    println!("cargo:rerun-if-env-changed=POSTHOG_HOST");

    // `transcribe-cpp-sys` asks the linker for `blas` alone, which is right on
    // macOS: Accelerate answers to that name and carries the CBLAS entry points
    // with it. Linux splits the two — reference `libblas` is the Fortran ABI and
    // defines no `cblas_*` at all, so `cblas_sgemm` and `cblas_sgemv` go
    // undefined at the final link of the binary, never at `cargo check`, which
    // does not link.
    //
    // Named here rather than patched into that crate, and emitted from the
    // *binary's* own build script so it lands last on the link line — which is
    // where an archive's undefined symbols get resolved from.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-lib=cblas");
    }

    tauri_build::build()
}
