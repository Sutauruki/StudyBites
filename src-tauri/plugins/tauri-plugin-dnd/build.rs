const COMMANDS: &[&str] = &[
    "dnd_status",
    "dnd_request_access",
    "dnd_enable",
    "dnd_restore",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
