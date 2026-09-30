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
            "clear_index",
            "reveal_in_os",
            "abs_path",
            "get_settings",
            "set_settings",
            "remember_selection",
            "set_root",
            "app_info",
            "open_data_dir",
            "check_update",
            "open_release",
            "smoke_report",
        ]),
    ))
    .expect("failed to run tauri-build");
}
