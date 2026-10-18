//! Test plugins: core modules in text, made into components against
//! the WIT file.

pub mod home;
pub mod tree;

use std::path::Path;

use wit_component::{ComponentEncoder, StringEncoding};
use wit_parser::Resolve;

pub use self::tree::{Item, Widget};

/// The parts every test plugin shares: the import of `log`, a memory,
/// a bump allocator for the canonical ABI, and a static `outgoing`
/// at 0 holding one `log-line` that reads "hello world" at 32.
const COMMON: &str = r#"
  (import "hamlet:plugin/host@0.1.0" "log" (func $log (param i32 i32 i32)))
  (import "hamlet:plugin/host@0.1.0" "panel"
    (func $panel (param i32 i32 i32 i32 i32 i32)))
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
  (data (i32.const 48) "\00\00\00\00\00\00\00\00")
"#;

/// A `panel-view` that returns a tree of no nodes.
const NO_VIEW: &str = r#"(func (export "hamlet:plugin/plugin@0.1.0#panel-view")
    (param i32 i32) (result i32)
    (i32.const 48))"#;

/// An `entry` that does nothing.
const QUIET: &str =
    r#"(func (export "hamlet:plugin/plugin@0.1.0#entry"))"#;

/// Declares the panel "main", draws `before` until a click reaches
/// its `step`, and `after` from then on.
pub fn drawing(before: &[Item], after: &[Item]) -> String {
    let before = tree::data(256, before);
    let after = tree::data(1536, after);
    format!(
        r#"(module {COMMON}
  {before}{after}
  (data (i32.const 176) "main")
  (data (i32.const 184) "MAIN")
  (data (i32.const 192) "book")
  (global $clicked (mut i32) (i32.const 0))
  (func (export "hamlet:plugin/plugin@0.1.0#entry")
    (call $panel (i32.const 176) (i32.const 4) (i32.const 184)
      (i32.const 4) (i32.const 192) (i32.const 4)))
  (func (export "hamlet:plugin/plugin@0.1.0#step")
    (param i32 i32) (result i32)
    (if (i32.gt_u (local.get 1) (i32.const 0))
      (then
        (if (i32.eq (i32.load8_u (local.get 0)) (i32.const 1))
          (then (global.set $clicked (i32.const 1))))))
    (i32.const 0))
  (func (export "hamlet:plugin/plugin@0.1.0#panel-view")
    (param i32 i32) (result i32)
    (if (result i32) (global.get $clicked)
      (then (i32.const 1536))
      (else (i32.const 256)))))"#
    )
}

/// Declares the panels "notes" and "again" (twice), with the icons
/// "book" and one that does not exist.
pub fn declaring() -> String {
    format!(
        r#"(module {COMMON}
  (data (i32.const 128) "notes")
  (data (i32.const 136) "NOTES")
  (data (i32.const 144) "book")
  (data (i32.const 152) "again")
  (data (i32.const 160) "nowhere")
  (func (export "hamlet:plugin/plugin@0.1.0#entry")
    (call $panel (i32.const 128) (i32.const 5) (i32.const 136)
      (i32.const 5) (i32.const 144) (i32.const 4))
    (call $panel (i32.const 152) (i32.const 5) (i32.const 136)
      (i32.const 5) (i32.const 160) (i32.const 7))
    (call $panel (i32.const 152) (i32.const 5) (i32.const 136)
      (i32.const 5) (i32.const 144) (i32.const 4)))
  (func (export "hamlet:plugin/plugin@0.1.0#step")
    (param i32 i32) (result i32)
    (i32.const 0))
  {NO_VIEW})"#
    )
}

/// Logs "logged" and answers with a line as long as the inbox.
pub fn echo() -> String {
    format!(
        r#"(module {COMMON} {QUIET} {NO_VIEW}
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
        r#"(module {COMMON} {QUIET} {NO_VIEW}
  (func (export "hamlet:plugin/plugin@0.1.0#step")
    (param i32 i32) (result i32)
    (loop $again (br $again))
    (i32.const 0)))"#
    )
}

/// Grows its memory until it is stopped.
pub fn grow() -> String {
    format!(
        r#"(module {COMMON} {QUIET} {NO_VIEW}
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
