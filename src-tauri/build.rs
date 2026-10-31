fn main() {
    // The LTK patcher host only finds the game from an elevated process, so Windows
    // starts Nebula as administrator, the way macOS relaunches it as root.
    // The manifest keeps tauri's default Common Controls v6 entry, which the dialogs need.
    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("windows-app-manifest.xml"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run tauri-build");

    // Embed Info.plist in the Mach-O binary so macOS Authorization Services
    // recognise this as a proper app (required for AuthorizationCreate).
    if cfg!(target_os = "macos") {
        let plist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Info.plist");
        println!(
            "cargo:rustc-link-arg=-sectcreate"
        );
        println!("cargo:rustc-link-arg=__TEXT");
        println!("cargo:rustc-link-arg=__info_plist");
        println!(
            "cargo:rustc-link-arg={}",
            plist.display()
        );
    }
}
