//! Test plugins: core modules in text, made into components against
//! the WIT file.

#[cfg(test)]
pub mod home;
pub mod tree;

use std::path::Path;

use wit_component::{ComponentEncoder, StringEncoding};
use wit_parser::Resolve;

pub use self::tree::{Item, Widget};

/// The parts every test plugin shares: the import of `log`, a memory,
/// a bump allocator for the canonical ABI, and a static `outgoing`
/// at 0 holding one `log-line` that reads "hello world" at 32. An
/// `intent` is 40 bytes with its payload at 8, and an `incoming` too.
const COMMON: &str = r#"
  (import "hamlet:plugin/host@0.5.0" "log" (func $log (param i32 i32 i32)))
  (import "hamlet:plugin/host@0.5.0" "command"
    (func $command (param i32 i32 i32 i32 i32 i32 i32)))
  (import "hamlet:plugin/host@0.5.0" "menu-item"
    (func $menu_item (param i32 i32 i32 i32 i32 i32 i32)))
  (import "hamlet:plugin/host@0.5.0" "context-item"
    (func $context_item (param i32 i32 i32 i32 i32 i32 i32)))
  (import "hamlet:plugin/host@0.5.0" "key"
    (func $key (param i32 i32 i32 i32)))
  (import "hamlet:plugin/host@0.5.0" "panel"
    (func $panel (param i32 i32 i32 i32 i32 i32)))
  (import "hamlet:plugin/host@0.5.0" "agents" (func $agents (param i32)))
  (import "hamlet:plugin/host@0.5.0" "papers" (func $papers (param i32)))
  (import "hamlet:plugin/host@0.5.0" "request"
    (func $request (param i32 i32 i32 i32) (result i32)))
  (import "hamlet:plugin/host@0.5.0" "storage-get"
    (func $storage_get (param i32 i32 i32)))
  (import "hamlet:plugin/host@0.5.0" "storage-set"
    (func $storage_set (param i32 i32 i32 i32 i32)))
  (import "hamlet:plugin/host@0.5.0" "state-get"
    (func $state_get (param i32)))
  (import "hamlet:plugin/host@0.5.0" "state-set"
    (func $state_set (param i32 i32 i32)))
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
  (data (i32.const 16) "\00\00\00\00\00\00\00\00\20\00\00\00\0b\00\00\00")
  (data (i32.const 32) "hello world")
  (data (i32.const 64) "logged")
  (data (i32.const 48) "\00\00\00\00\00\00\00\00\00\00\00\00")
"#;

/// A `panel-view` that returns a tree of no nodes.
const NO_VIEW: &str = r#"(func (export "hamlet:plugin/plugin@0.5.0#panel-view")
    (param i32 i32) (result i32)
    (i32.const 48))"#;

/// An `entry` that does nothing.
const QUIET: &str =
    r#"(func (export "hamlet:plugin/plugin@0.5.0#entry"))"#;

/// What a `step` does when the first thing in its inbox is a click.
const ON_CLICK: &str = "(global.set $clicked (i32.const 1))";

/// Declares the panel "main", draws `before` until a click reaches
/// its `step`, and `after` from then on.
pub fn drawing(before: &[Item], after: &[Item]) -> String {
    panelled(ON_CLICK, before, after)
}

/// Declares the panel "main" and draws `tree`. A click that reaches
/// its `step` traps it.
pub fn failing(tree: &[Item]) -> String {
    panelled("unreachable", tree, tree)
}

/// Declares the panel "main" and draws the program `text`.
pub fn speaking(text: &str) -> String {
    let view = tree::lang(256, text);
    format!(
        r#"(module {COMMON}
  {view}
  (data (i32.const 176) "main")
  (data (i32.const 184) "MAIN")
  (data (i32.const 192) "book")
  (func (export "hamlet:plugin/plugin@0.5.0#entry")
    (call $panel (i32.const 176) (i32.const 4) (i32.const 184)
      (i32.const 4) (i32.const 192) (i32.const 4)))
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (i32.const 0))
  (func (export "hamlet:plugin/plugin@0.5.0#panel-view")
    (param i32 i32) (result i32)
    (i32.const 256)))"#
    )
}

fn panelled(
    on_click: &str,
    before: &[Item],
    after: &[Item],
) -> String {
    let before = tree::data(256, before);
    let after = tree::data(1536, after);
    format!(
        r#"(module {COMMON}
  {before}{after}
  (data (i32.const 176) "main")
  (data (i32.const 184) "MAIN")
  (data (i32.const 192) "book")
  (global $clicked (mut i32) (i32.const 0))
  (func (export "hamlet:plugin/plugin@0.5.0#entry")
    (call $panel (i32.const 176) (i32.const 4) (i32.const 184)
      (i32.const 4) (i32.const 192) (i32.const 4)))
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (if (i32.gt_u (local.get 1) (i32.const 0))
      (then
        (if (i32.eq (i32.load8_u (local.get 0)) (i32.const 1))
          (then {on_click}))))
    (i32.const 0))
  (func (export "hamlet:plugin/plugin@0.5.0#panel-view")
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
  (func (export "hamlet:plugin/plugin@0.5.0#entry")
    (call $panel (i32.const 128) (i32.const 5) (i32.const 136)
      (i32.const 5) (i32.const 144) (i32.const 4))
    (call $panel (i32.const 152) (i32.const 5) (i32.const 136)
      (i32.const 5) (i32.const 160) (i32.const 7))
    (call $panel (i32.const 152) (i32.const 5) (i32.const 136)
      (i32.const 5) (i32.const 144) (i32.const 4)))
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (i32.const 0))
  {NO_VIEW})"#
    )
}

/// Declares the panel "main", and in its first `step` asks for the
/// panel `panel` to be opened.
pub fn opening(panel: &str) -> String {
    let len = panel.len();
    format!(
        r#"(module {COMMON} {NO_VIEW}
  (data (i32.const 176) "main")
  (data (i32.const 184) "MAIN")
  (data (i32.const 192) "book")
  (data (i32.const 600) "{panel}")
  (global $sent (mut i32) (i32.const 0))
  (func (export "hamlet:plugin/plugin@0.5.0#entry")
    (call $panel (i32.const 176) (i32.const 4) (i32.const 184)
      (i32.const 4) (i32.const 192) (i32.const 4)))
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (if (result i32) (global.get $sent)
      (then {NOTHING})
      (else
        (global.set $sent (i32.const 1))
        (i32.store (i32.const 512) (i32.const 528))
        (i32.store (i32.const 516) (i32.const 1))
        (i32.store8 (i32.const 528) (i32.const 5))
        (i32.store (i32.const 536) (i32.const 600))
        (i32.store (i32.const 540) (i32.const {len}))
        (i32.const 512)))))"#
    )
}

/// Reads the agents in `step`. It answers with a line as long as the
/// list is, or logs the error it got as an error.
pub fn reading() -> String {
    format!(
        r#"(module {COMMON} {QUIET} {NO_VIEW}
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (call $agents (i32.const 96))
    (if (i32.eqz (i32.load8_u (i32.const 96)))
      (then (i32.store (i32.const 28) (i32.load (i32.const 104))))
      (else
        (call $log (i32.const 2) (i32.load (i32.const 100))
          (i32.load (i32.const 104)))
        (i32.store (i32.const 28) (i32.const 0))))
    (i32.const 0)))"#
    )
}

/// Reads the papers in `step`. It answers with a line as long as the
/// notes are, or logs the error it got as an error.
pub fn reading_papers() -> String {
    format!(
        r#"(module {COMMON} {QUIET} {NO_VIEW}
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (call $papers (i32.const 96))
    (if (i32.eqz (i32.load8_u (i32.const 96)))
      (then (i32.store (i32.const 28) (i32.load (i32.const 104))))
      (else
        (call $log (i32.const 2) (i32.load (i32.const 100))
          (i32.load (i32.const 104)))
        (i32.store (i32.const 28) (i32.const 0))))
    (i32.const 0)))"#
    )
}

/// Asks for `capability` in `step`, with the reason "why", and
/// answers with a line as long as the answer is: 0 granted, 1 denied,
/// 2 pending.
pub fn requesting(capability: &str) -> String {
    let len = capability.len();
    format!(
        r#"(module {COMMON} {QUIET} {NO_VIEW}
  (data (i32.const 80) "{capability}")
  (data (i32.const 112) "why")
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (i32.store (i32.const 28)
      (call $request (i32.const 80) (i32.const {len})
        (i32.const 112) (i32.const 3)))
    (i32.const 0)))"#
    )
}

/// Looks for the key "note" in its storage in `step`. It answers
/// with a line as long as the value it found, and where there is none
/// it stores "hello world" and answers with an empty line.
pub fn keeping() -> String {
    format!(
        r#"(module {COMMON} {QUIET} {NO_VIEW}
  (data (i32.const 120) "note")
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (call $storage_get (i32.const 120) (i32.const 4) (i32.const 96))
    (if (i32.load8_u (i32.const 96))
      (then (i32.store (i32.const 28) (i32.load (i32.const 104))))
      (else
        (call $storage_set (i32.const 120) (i32.const 4)
          (i32.const 32) (i32.const 11) (i32.const 96))
        (i32.store (i32.const 28) (i32.const 0))))
    (i32.const 0)))"#
    )
}

/// Answers in `step` with a line as long as the state the host holds
/// for it, then sets the state to the first `keep` bytes of "logged".
pub fn remembering(keep: u32) -> String {
    format!(
        r#"(module {COMMON} {QUIET} {NO_VIEW}
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (call $state_get (i32.const 96))
    (i32.store (i32.const 28) (i32.load (i32.const 100)))
    (call $state_set (i32.const 64) (i32.const {keep}) (i32.const 96))
    (i32.const 0)))"#
    )
}

/// Logs "logged" and answers with a line as long as the inbox.
pub fn echo() -> String {
    format!(
        r#"(module {COMMON} {QUIET} {NO_VIEW}
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (call $log (i32.const 0) (i32.const 64) (i32.const 6))
    (i32.store (i32.const 28) (local.get 1))
    (i32.const 0)))"#
    )
}

/// An `outgoing` with no intent, at 48.
const NOTHING: &str = "(i32.const 48)";

/// Runs the command `command` of the plugin `plugin` with `args` in
/// its first `step`, as call 7, and logs what each reply to it holds:
/// at info for what the command returned, at error for why not.
pub fn commanding(plugin: &str, command: &str, args: &str) -> String {
    let (plugin_len, command_len, args_len) =
        (plugin.len(), command.len(), args.len());
    format!(
        r#"(module {COMMON} {QUIET} {NO_VIEW}
  (data (i32.const 600) "{plugin}")
  (data (i32.const 640) "{command}")
  (data (i32.const 680) "{args}")
  (global $sent (mut i32) (i32.const 0))
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (local $at i32) (local $end i32) (local $out i32)
    (local.set $out {NOTHING})
    (if (i32.eqz (global.get $sent))
      (then
        (global.set $sent (i32.const 1))
        (i32.store (i32.const 512) (i32.const 528))
        (i32.store (i32.const 516) (i32.const 1))
        (i32.store8 (i32.const 528) (i32.const 2))
        (i64.store (i32.const 536) (i64.const 7))
        (i32.store (i32.const 544) (i32.const 600))
        (i32.store (i32.const 548) (i32.const {plugin_len}))
        (i32.store (i32.const 552) (i32.const 640))
        (i32.store (i32.const 556) (i32.const {command_len}))
        (i32.store (i32.const 560) (i32.const 680))
        (i32.store (i32.const 564) (i32.const {args_len}))
        (local.set $out (i32.const 512))))
    (local.set $at (local.get 0))
    (local.set $end
      (i32.add (local.get 0) (i32.mul (local.get 1) (i32.const 40))))
    (block $done
      (loop $each
        (br_if $done (i32.ge_u (local.get $at) (local.get $end)))
        (if (i32.eq (i32.load8_u (local.get $at)) (i32.const 7))
          (then
            (call $log
              (select (i32.const 0) (i32.const 2)
                (i32.eqz
                  (i32.load8_u (i32.add (local.get $at) (i32.const 16)))))
              (i32.load (i32.add (local.get $at) (i32.const 20)))
              (i32.load (i32.add (local.get $at) (i32.const 24))))))
        (local.set $at (i32.add (local.get $at) (i32.const 40)))
        (br $each)))
    (local.get $out)))"#
    )
}

/// Declares the command "go", a line "Go" of the VIEW menu that runs
/// it, an item "Go" of the menu of an agent and the shortcut Cmd+G
/// for it, and logs "hello world" at info each time it is given a
/// command.
pub fn contributing() -> String {
    format!(
        r#"(module {COMMON} {NO_VIEW}
  (data (i32.const 600) "go")
  (data (i32.const 640) "Go")
  (data (i32.const 660) "view")
  (data (i32.const 680) "Cmd+G")
  (data (i32.const 700) "agent")
  (func (export "hamlet:plugin/plugin@0.5.0#entry")
    (call $command (i32.const 600) (i32.const 2) (i32.const 640)
      (i32.const 2) (i32.const 0) (i32.const 0) (i32.const 0))
    (call $menu_item (i32.const 660) (i32.const 4) (i32.const 3)
      (i32.const 640) (i32.const 2) (i32.const 600) (i32.const 2))
    (call $context_item (i32.const 700) (i32.const 5) (i32.const 0)
      (i32.const 640) (i32.const 2) (i32.const 600) (i32.const 2))
    (call $key (i32.const 680) (i32.const 5) (i32.const 600)
      (i32.const 2)))
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (local $at i32) (local $end i32)
    (local.set $at (local.get 0))
    (local.set $end
      (i32.add (local.get 0) (i32.mul (local.get 1) (i32.const 40))))
    (block $done
      (loop $each
        (br_if $done (i32.ge_u (local.get $at) (local.get $end)))
        (if (i32.eq (i32.load8_u (local.get $at)) (i32.const 6))
          (then (call $log (i32.const 0) (i32.const 32) (i32.const 11))))
        (local.set $at (i32.add (local.get $at) (i32.const 40)))
        (br $each)))
    {NOTHING}))"#
    )
}

/// Declares the command "add", and answers each command it is given
/// with the arguments it came with.
pub fn answering() -> String {
    format!(
        r#"(module {COMMON} {NO_VIEW}
  (data (i32.const 600) "add")
  (data (i32.const 640) "Add")
  (func (export "hamlet:plugin/plugin@0.5.0#entry")
    (call $command (i32.const 600) (i32.const 3) (i32.const 640)
      (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 0)))
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (local $at i32) (local $end i32) (local $out i32)
    (local.set $out {NOTHING})
    (local.set $at (local.get 0))
    (local.set $end
      (i32.add (local.get 0) (i32.mul (local.get 1) (i32.const 40))))
    (block $done
      (loop $each
        (br_if $done (i32.ge_u (local.get $at) (local.get $end)))
        (if (i32.eq (i32.load8_u (local.get $at)) (i32.const 6))
          (then
            (i32.store (i32.const 512) (i32.const 528))
            (i32.store (i32.const 516) (i32.const 1))
            (i32.store8 (i32.const 528) (i32.const 3))
            (i64.store (i32.const 536)
              (i64.load (i32.add (local.get $at) (i32.const 8))))
            (i32.store8 (i32.const 544) (i32.const 0))
            (i32.store (i32.const 548)
              (i32.load (i32.add (local.get $at) (i32.const 32))))
            (i32.store (i32.const 552)
              (i32.load (i32.add (local.get $at) (i32.const 36))))
            (local.set $out (i32.const 512))))
        (local.set $at (i32.add (local.get $at) (i32.const 40)))
        (br $each)))
    (local.get $out)))"#
    )
}

/// Publishes the event `topic` with `payload` in its first `step`.
pub fn publishing(topic: &str, payload: &str) -> String {
    let (topic_len, payload_len) = (topic.len(), payload.len());
    format!(
        r#"(module {COMMON} {QUIET} {NO_VIEW}
  (data (i32.const 600) "{topic}")
  (data (i32.const 680) "{payload}")
  (global $sent (mut i32) (i32.const 0))
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (if (global.get $sent) (then {NOTHING} return))
    (global.set $sent (i32.const 1))
    (i32.store (i32.const 512) (i32.const 528))
    (i32.store (i32.const 516) (i32.const 1))
    (i32.store8 (i32.const 528) (i32.const 4))
    (i32.store (i32.const 536) (i32.const 600))
    (i32.store (i32.const 540) (i32.const {topic_len}))
    (i32.store (i32.const 544) (i32.const 680))
    (i32.store (i32.const 548) (i32.const {payload_len}))
    (i32.const 512)))"#
    )
}

/// Logs the payload of each event it is given, at info.
pub fn listening() -> String {
    format!(
        r#"(module {COMMON} {QUIET} {NO_VIEW}
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (local $at i32) (local $end i32)
    (local.set $at (local.get 0))
    (local.set $end
      (i32.add (local.get 0) (i32.mul (local.get 1) (i32.const 40))))
    (block $done
      (loop $each
        (br_if $done (i32.ge_u (local.get $at) (local.get $end)))
        (if (i32.eq (i32.load8_u (local.get $at)) (i32.const 8))
          (then
            (call $log (i32.const 0)
              (i32.load (i32.add (local.get $at) (i32.const 24)))
              (i32.load (i32.add (local.get $at) (i32.const 28))))))
        (local.set $at (i32.add (local.get $at) (i32.const 40)))
        (br $each)))
    {NOTHING}))"#
    )
}

/// Does nothing in `step`, and never answers a command.
pub fn quiet() -> String {
    format!(
        r#"(module {COMMON} {QUIET} {NO_VIEW}
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    {NOTHING}))"#
    )
}

/// Never returns.
pub fn spin() -> String {
    format!(
        r#"(module {COMMON} {QUIET} {NO_VIEW}
  (func (export "hamlet:plugin/plugin@0.5.0#step")
    (param i32 i32) (result i32)
    (loop $again (br $again))
    (i32.const 0)))"#
    )
}

/// Grows its memory until it is stopped.
pub fn grow() -> String {
    format!(
        r#"(module {COMMON} {QUIET} {NO_VIEW}
  (func (export "hamlet:plugin/plugin@0.5.0#step")
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

/// A component built for an earlier version of the interface.
pub fn outdated() -> Vec<u8> {
    wat::parse_str(
        r#"(component
  (import "hamlet:plugin/host@0.1.0" (instance (export "f" (func)))))"#,
    )
    .expect("the outdated component is valid")
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
