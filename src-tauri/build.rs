fn main() {
    #[cfg(feature = "app")]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_root",
            "list_directory",
            "list_directories",
            "get_children",
            "expand_directory",
            "get_file_metadata",
            "search_files",
            "index_status",
            "start_indexing",
            "reveal_path",
            "open_path",
        ]),
    ))
    .expect("failed to run tauri-build");
}
