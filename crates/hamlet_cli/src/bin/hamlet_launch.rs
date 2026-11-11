//! The program a packaged Hamlet opens: the server, unless it serves,
//! and the window. Opened from a menu, it asks Windows for no
//! console.

#![windows_subsystem = "windows"]

fn main() {
    std::process::exit(hamlet_cli::launch::run());
}
