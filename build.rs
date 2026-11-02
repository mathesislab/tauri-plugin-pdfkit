const COMMANDS: &[&str] = &["open", "frame", "cover", "close", "find", "zoom", "go"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
