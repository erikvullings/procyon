//! Installs or removes a private exact-production macOS semantic qualification profile.

use std::error::Error;
use std::path::PathBuf;

use fm_semantic_components::{QualificationProfile, install_macos_qualification};

fn main() {
    if let Err(error) = run() {
        eprintln!("semantic qualification kit: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [
            command,
            dedicated_home,
            profile_root,
            catalog,
            signature,
            artifacts,
            public_key,
        ] if command == "install" => {
            require_current_home(&PathBuf::from(dedicated_home))?;
            let receipt = install_macos_qualification(
                &PathBuf::from(dedicated_home),
                &PathBuf::from(profile_root),
                &PathBuf::from(catalog),
                &PathBuf::from(signature),
                &PathBuf::from(artifacts),
                &PathBuf::from(public_key),
            )?;
            println!(
                "installed {} signed artifacts into {}",
                receipt.installed_artifact_count(),
                receipt.application_data().display()
            );
            Ok(())
        }
        [command, dedicated_home, profile_root] if command == "cleanup" => {
            require_current_home(&PathBuf::from(dedicated_home))?;
            QualificationProfile::open(PathBuf::from(dedicated_home), PathBuf::from(profile_root))?
                .cleanup()?;
            println!("removed qualification profile {}", profile_root.display());
            Ok(())
        }
        _ => Err(
            "usage:\n  semantic_qualification_kit install <dedicated-home> <profile-root> \
             <catalog.json> <catalog.sig> <artifacts> <catalog.pub>\n  \
             semantic_qualification_kit cleanup <dedicated-home> <profile-root>"
                .into(),
        ),
    }
}

fn require_current_home(dedicated_home: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let current_home =
        std::env::var_os("HOME").ok_or("HOME is required for qualification profile isolation")?;
    if std::fs::canonicalize(current_home)? != std::fs::canonicalize(dedicated_home)? {
        return Err("dedicated-home must be the current dedicated macOS test user's HOME".into());
    }
    Ok(())
}
