fn main() {
    // No JavaScript plugin commands: the application authorizes sessions in Rust first.
    tauri_plugin::Builder::new(&[])
        .android_path("android")
        .build();
}
