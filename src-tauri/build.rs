fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "app_status",
            "unlock",
            "lock",
            "exit_app",
            "query_countries",
            "locate_channel",
            "list_jobs",
            "list_sources",
            "save_source",
            "remove_source",
            "refresh_source",
            "job_status",
            "cancel_job",
            "query_channels",
            "query_groups",
            "query_guide",
            "set_favorite",
            "set_epg_mapping",
            "open_playback",
            "play_native",
            "close_playback",
        ]),
    ))
    .expect("Tauri build configuration is invalid");
}
