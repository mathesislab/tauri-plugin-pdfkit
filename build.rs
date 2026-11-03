const COMMANDS: &[&str] = &[
    "open",
    "frame",
    "cover",
    "close",
    "find",
    "zoom",
    "go",
    "back",
    "forward",
    "page_to",
    "turn",
    "selection",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
