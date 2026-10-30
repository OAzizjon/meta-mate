fn main() {
    // Tauri embeds its manifest (Common Controls v6) in the app binary only; the test binary
    // then stops at load with STATUS_ENTRYPOINT_NOT_FOUND (TaskDialogIndirect). The same
    // manifest goes into every binary instead (tauri-build's documented recipe).
    let mut attributes = tauri_build::Attributes::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        let dir = std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo");
        let manifest = std::path::Path::new(&dir).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::try_build(attributes).expect("tauri build script");
}
