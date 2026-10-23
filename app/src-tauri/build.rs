include!("src/command_names.rs");

fn main() {
    // Declaring the commands generates an allow/deny permission for each,
    // so a command is only callable if capabilities/default.json allows it.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
