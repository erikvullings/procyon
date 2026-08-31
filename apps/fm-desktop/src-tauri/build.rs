//! Runs `tauri-build`'s codegen (capabilities schema, Windows resources).
fn main() {
    // `tauri-build` embeds a manifest requesting the side-by-side comctl32 v6
    // assembly, but only for the app binary. Test harnesses link the same GUI
    // stack - which imports `TaskDialogIndirect`, exported by v6 only - so
    // without the same dependency the loader binds system32's v5.82 and the
    // process dies with STATUS_ENTRYPOINT_NOT_FOUND before any test runs.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' \
             name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
    tauri_build::build();
}
