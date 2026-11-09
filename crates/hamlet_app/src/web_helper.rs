//! The process the web engine starts for the Web panels' pages: their
//! renderers, their GPU and their network (ADR 0014).

#[cfg(target_os = "macos")]
#[path = "screens/web/engine.rs"]
mod engine;

#[cfg(target_os = "macos")]
fn main() {
    use bevy_cef_core::prelude::{
        CustomLibraryLoader, RenderProcessAppBuilder,
        resolve_custom_cef_paths,
    };
    use cef::args::Args;
    use cef::{api_hash, execute_process, sys};

    let args = Args::new();
    let located = engine::located();
    let (_, binary, _) =
        resolve_custom_cef_paths(located.as_deref(), None);
    let Some(binary) = binary else {
        std::process::exit(1);
    };
    let loader = CustomLibraryLoader::new(binary);
    if !loader.load() {
        std::process::exit(1);
    }
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let mut app = RenderProcessAppBuilder::build();
    let code = execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    drop(loader);
    std::process::exit(code);
}

#[cfg(not(target_os = "macos"))]
fn main() {}
