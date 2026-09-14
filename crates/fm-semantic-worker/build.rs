//! Adds a relocatable native-library lookup path to semantic worker binaries.

fn main() {
    if std::env::var_os("CARGO_FEATURE_SEMANTIC_RUNTIME").is_none() {
        return;
    }
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => {
            println!("cargo:rustc-link-arg-bin=fm-semantic-worker=-Wl,-rpath,@loader_path");
        }
        Ok("linux") => {
            println!("cargo:rustc-link-arg-bin=fm-semantic-worker=-Wl,-rpath,$ORIGIN");
        }
        _ => {}
    }
}
