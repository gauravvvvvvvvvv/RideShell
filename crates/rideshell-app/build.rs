fn main() {
    println!("cargo:rerun-if-changed=icon.ico");
    println!("cargo:rerun-if-env-changed=RIDESHELL_TELEMETRY_URL");
    println!("cargo:rerun-if-env-changed=RIDESHELL_PROMO_URL");
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("app.rc", embed_resource::NONE).manifest_optional().unwrap();
    }
}
