//! Bundles RideShell App (`rideshell-app.exe`) into `rideshell.exe` when
//! `RIDESHELL_APP_EXE` points at a built copy (see scripts/build-release.ps1),
//! so a single download installs both. Also embeds the icon on Windows.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=RIDESHELL_APP_EXE");
    println!("cargo:rerun-if-changed=cli.rc");
    println!("cargo:rerun-if-changed=../rideshell-app/icon.ico");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app_bundle.rs");
    let code = match std::env::var("RIDESHELL_APP_EXE") {
        Ok(p) if Path::new(&p).is_file() => {
            println!("cargo:rerun-if-changed={p}");
            format!("pub const APP_EXE: Option<&[u8]> = Some(include_bytes!({p:?}));\n")
        }
        _ => "pub const APP_EXE: Option<&[u8]> = None;\n".to_string(),
    };
    std::fs::write(out, code).unwrap();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("cli.rc", embed_resource::NONE).manifest_optional().unwrap();
    }
}
