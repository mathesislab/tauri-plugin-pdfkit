const COMMANDS: &[&str] = &[
    "open",
    "frame",
    "cover",
    "close",
    "find",
    "zoom",
    "go",
    "selection",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
