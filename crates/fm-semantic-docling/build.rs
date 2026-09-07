//! Prevents Docling's transitive ONNX dependency from downloading binaries.

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_ML");
    println!("cargo:rerun-if-env-changed=CARGO_NET_OFFLINE");
    println!("cargo:rerun-if-env-changed=ORT_OFFLINE");
    println!("cargo:rerun-if-env-changed=ORT_SKIP_DOWNLOAD");

    if std::env::var_os("CARGO_FEATURE_ML").is_some()
        && !offline("CARGO_NET_OFFLINE")
        && !offline("ORT_OFFLINE")
        && !offline("ORT_SKIP_DOWNLOAD")
    {
        panic!(
            "fm-semantic-docling's ml feature requires ORT_SKIP_DOWNLOAD=1; \
             release packaging must supply a checksum-verified ONNX Runtime"
        );
    }
}

fn offline(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|value| {
        !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
    })
}
