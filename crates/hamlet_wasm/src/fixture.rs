//! Test plugins: core modules in text, made into components against
//! the WIT file.

use std::path::Path;

use wit_component::{ComponentEncoder, StringEncoding};
use wit_parser::Resolve;

/// The parts every test plugin shares: the import of `log`, a memory,
/// a bump allocator for the canonical ABI, and a static `outgoing`
/// at 0 holding one `log-line` that reads "hello world" at 32.
const COMMON: &str = r#"
  (import "hamlet:plugin/host@0.1.0" "log" (func $log (param i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $ptr i32)
    (local.set $ptr
      (i32.and
        (i32.add (global.get $heap) (i32.sub (local.get 2) (i32.const 1)))
        (i32.sub (i32.const 0) (local.get 2))))
    (global.set $heap (i32.add (local.get $ptr) (local.get 3)))
    (local.get $ptr))
  (data (i32.const 0) "\10\00\00\00\01\00\00\00")
  (data (i32.const 16) "\00\00\00\00\20\00\00\00\0b\00\00\00")
  (data (i32.const 32) "hello world")
  (data (i32.const 64) "logged")
  (func (export "hamlet:plugin/plugin@0.1.0#entry"))
"#;

/// Logs "logged" and answers with a line as long as the inbox.
pub fn echo() -> String {
    format!(
        r#"(module {COMMON}
  (func (export "hamlet:plugin/plugin@0.1.0#step")
    (param i32 i32) (result i32)
    (call $log (i32.const 0) (i32.const 64) (i32.const 6))
    (i32.store (i32.const 24) (local.get 1))
    (i32.const 0)))"#
    )
}

/// Never returns.
pub fn spin() -> String {
    format!(
        r#"(module {COMMON}
  (func (export "hamlet:plugin/plugin@0.1.0#step")
    (param i32 i32) (result i32)
    (loop $again (br $again))
    (i32.const 0)))"#
    )
}

/// Grows its memory until it is stopped.
pub fn grow() -> String {
    format!(
        r#"(module {COMMON}
  (func (export "hamlet:plugin/plugin@0.1.0#step")
    (param i32 i32) (result i32)
    (loop $again
      (drop (memory.grow (i32.const 16)))
      (br $again))
    (i32.const 0)))"#
    )
}

/// A component that imports an interface the host does not define.
pub fn greedy() -> Vec<u8> {
    wat::parse_str(
        r#"(component
  (import "greedy:thing/stuff" (instance (export "f" (func)))))"#,
    )
    .expect("the greedy component is valid")
}

/// The component of a core module in text, as the WIT file describes
/// it.
pub fn component(module: &str) -> Vec<u8> {
    let mut resolve = Resolve::default();
    let (package, _) = resolve
        .push_path(Path::new(env!("CARGO_MANIFEST_DIR")).join("wit"))
        .expect("the WIT file parses");
    let world = resolve
        .select_world(&[package], Some("hamlet"))
        .expect("the world is there");
    let mut core =
        wat::parse_str(module).expect("the module is valid");
    wit_component::embed_component_metadata(
        &mut core,
        &resolve,
        world,
        StringEncoding::UTF8,
        false,
    )
    .expect("the metadata embeds");
    ComponentEncoder::default()
        .module(&core)
        .expect("the module is valid")
        .validate(true)
        .encode()
        .expect("the component encodes")
}
