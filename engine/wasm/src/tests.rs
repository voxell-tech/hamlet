//! Plugins written as core modules in text, made into components
//! against the WIT file, so no test needs a WASM toolchain.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use bevy::app::{App, TaskPoolPlugin, Update};
use bevy::asset::{AssetApp, AssetPlugin};
use bevy::camera::{
    Camera, Camera2d, ComputedCameraValues, RenderTargetInfo,
};
use bevy::ecs::entity::Entity;
use bevy::ecs::error::ignore;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::image::Image;
use bevy::input::InputPlugin;
use bevy::math::UVec2;
use bevy::picking::{InteractionPlugin, PickingPlugin};
use bevy::text::{FontCx, ScaleCx, TextPipeline};
use bevy::time::{TimePlugin, TimeUpdateStrategy};
use bevy::transform::TransformPlugin;
use bevy::ui::{ComputedNode, UiPlugin, px};
use bevy_fynix::mount;
use bevy_fynix::views::{FrameProps, column};
use hamlet_engine_api::{
    API_VERSION, Capability, PluginId, PluginLimits,
};
use hamlet_engine_core::tuning::Tuning;
use hamlet_engine_core::{Budgets, Bundle, Home, LogLevel};
use hamlet_engine_grants::{
    Admitted, Answer, Grants, Petition, Record,
};
use hamlet_engine_plugins::{ContributedBy, Stopped, Uninstall};
use hamlet_engine_shell::EngineTheme;
use wasmtime::component::Resource;
use wit_component::{ComponentEncoder, StringEncoding};
use wit_parser::Resolve;

use crate::add::check;
use crate::budget::Spending;
use crate::folder::{FOLDER, MANIFEST, WASM};
use crate::host::{Host, WasmTuning};
use crate::instance::ui_bindings::hamlet::engine::ui::{
    Fill, HostNode, Kind, Property,
};
use crate::instance::{Caps, Fault, Instance, State, link};
use crate::nodes::{NodeChange, NodeError};
use crate::panel::{Inbox, Surface, Work};
use crate::retained::{NodeOf, UiRoot, WasmWorldExt, find_scrolls};

/// What `run` does in a test plugin that counts its runs.
const COUNT: &str =
    "(global.set $count (i32.add (global.get $count) (i32.const 1)))";

/// The exports every test plugin has, with `declare` and `run` doing
/// what they are given. The return area at 0 points at an empty list,
/// which is no `Contribution`s; the one at 32 at the `Contribution`s
/// of one panel, "p", whose name is also at 64. The count, which
/// [`COUNT`] counts, is its state.
fn module(declare: &str, run: &str) -> String {
    format!(
        r#"(module
  (import "hamlet:engine/host@0.1.0" "log" (func $log (param i32 i32 i32)))
  (memory (export "memory") 1)
  (memory $more 1)
  (global $heap (mut i32) (i32.const 4096))
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $at i32)
    (local.set $at
      (i32.and
        (i32.add (global.get $heap) (i32.sub (local.get 2) (i32.const 1)))
        (i32.sub (i32.const 0) (local.get 2))))
    (global.set $heap (i32.add (local.get $at) (local.get 3)))
    (local.get $at))
  (data (i32.const 0) "\08\00\00\00\01\00\00\00\00")
  (data (i32.const 16) "\01\00\01p\01P\00\00")
  (data (i32.const 32) "\10\00\00\00\08\00\00\00")
  (data (i32.const 64) "p")
  (func (export "hamlet:engine/guest@0.1.0#declare") (result i32)
    {declare})
  (func (export "hamlet:engine/guest@0.1.0#receive") (param i32 i32))
  (func (export "hamlet:engine/guest@0.1.0#serve")
    (param i32 i32 i32 i32 i32 i32) (result i32)
    (i32.const 256))
  (global $count (mut i32) (i32.const 0))
  (func (export "hamlet:engine/guest@0.1.0#run") (param i32 i32)
    {run})
  (func (export "hamlet:engine/guest@0.1.0#save") (result i32)
    (i32.store (i32.const 128) (global.get $count))
    (i32.store (i32.const 96) (i32.const 128))
    (i32.store (i32.const 100) (i32.const 4))
    (i32.const 96))
  (func (export "hamlet:engine/guest@0.1.0#restore") (param i32 i32)
    (if (i32.eq (local.get 1) (i32.const 4))
      (then (global.set $count (i32.load (local.get 0)))))))"#
    )
}

/// The parts of WASI 0.2 a test plugin uses, and a world of the
/// plugin's with them.
const WASI: [&str; 5] = [
    "package wasi:io@0.2.0;
    interface error { resource error; }
    interface poll { resource pollable { block: func(); } }
    interface streams {
      use error.{error};
      variant stream-error { last-operation-failed(error), closed }
      resource output-stream {
        blocking-write-and-flush: func(contents: list<u8>) -> \
     result<_, stream-error>;
      }
    }",
    "package wasi:clocks@0.2.0;
    interface monotonic-clock {
      use wasi:io/poll@0.2.0.{pollable};
      now: func() -> u64;
      subscribe-duration: func(when: u64) -> pollable;
    }",
    "package wasi:random@0.2.0;
    interface random { get-random-u64: func() -> u64; }",
    "package wasi:cli@0.2.0;
    interface stdout {
      use wasi:io/streams@0.2.0.{output-stream};
      get-stdout: func() -> output-stream;
    }",
    "package test:system;
    world plugin {
      include hamlet:engine/plugin@0.1.0;
      import wasi:cli/stdout@0.2.0;
      import wasi:clocks/monotonic-clock@0.2.0;
      import wasi:random/random@0.2.0;
    }",
];

/// The component of the core module in `text`, with the imports of
/// [`WASI`] when `wasi`.
fn component(text: &str, wasi: bool) -> Vec<u8> {
    let mut resolve = Resolve::default();
    let mut package = resolve
        .push_str(
            "hamlet_engine.wit",
            include_str!("../../wit/hamlet_engine.wit"),
        )
        .expect("the WIT parses");
    if wasi {
        for text in WASI {
            package = resolve
                .push_str("wasi.wit", text)
                .expect("the WIT parses");
        }
    }
    let world = resolve
        .select_world(&[package], Some("plugin"))
        .expect("the world is there");
    let mut core = wat::parse_str(text).expect("the module parses");
    wit_component::embed_component_metadata(
        &mut core,
        &resolve,
        world,
        StringEncoding::UTF8,
        false,
    )
    .expect("the WIT embeds");
    ComponentEncoder::default()
        .module(&core)
        .expect("the module is one")
        .validate(true)
        .encode()
        .expect("the component encodes")
}

/// The limits of the tests: a plugin of four pages of memory.
const LIMITS: PluginLimits = PluginLimits { memory: 4 * 65_536 };

/// The budgets of the tests: a kilobyte handed over a call.
fn budgets() -> Budgets {
    Budgets {
        bytes_per_call: 1_024,
        ..Budgets::designed()
    }
}

/// The plugin `component`, started within `limits` and `budgets`,
/// with its caps.
fn start(
    component: &[u8],
    limits: &PluginLimits,
    budgets: &Budgets,
) -> (Instance, Caps) {
    let tuning = WasmTuning {
        plugins: 1,
        ..WasmTuning::designed()
    };
    let host = Host::new(&tuning).expect("the host starts");
    let linked = link(&host.0, component).expect("the plugin links");
    let caps = Caps::of(limits, budgets, &tuning);
    let instance = Instance::new(&host.0, &linked, &caps)
        .expect("the plugin starts");
    (instance, caps)
}

/// The plugin whose `declare` does `declare`, made into a component.
fn declaring(declare: &str) -> Vec<u8> {
    component(&module(declare, COUNT), false)
}

#[test]
fn nodes_are_held_to_their_budgets_and_kinds() {
    let caps = Caps {
        node_changes: 6,
        node_depth: 1,
        ..Caps::of(&LIMITS, &budgets(), &WasmTuning::designed())
    };
    let mut state = State::new(&caps);
    let root = state.nodes.root().rep();
    let make = |state: &mut State, kind, parent: u32| {
        HostNode::new(state, kind, Resource::new_borrow(parent), 0)
            .map(|node| node.rep())
    };
    let set = |state: &mut State, node: u32, value| {
        HostNode::set(state, Resource::new_borrow(node), value, None)
    };
    let refused = |error: wasmtime::Error| match Fault::of(error) {
        Fault::Node(why) => why,
        fault => panic!("{fault}"),
    };

    let label = make(&mut state, Kind::Label, root).unwrap();
    let under_label = make(&mut state, Kind::Label, label);
    assert_eq!(refused(under_label.unwrap_err()), NodeError::Leaf);
    let placeholder = Property::Placeholder("A name".into());
    let wrong = set(&mut state, label, placeholder);
    assert_eq!(refused(wrong.unwrap_err()), NodeError::Property);
    let frame = make(&mut state, Kind::Frame, root).unwrap();
    let deep = make(&mut state, Kind::Label, frame);
    assert_eq!(refused(deep.unwrap_err()), NodeError::Depth);
    assert!(set(&mut state, label, Property::Bold(true)).is_ok());
    let past = set(&mut state, label, Property::Bold(false));
    assert_eq!(refused(past.unwrap_err()), NodeError::Changes);
}

#[test]
fn a_call_hands_over_its_bytes_in_all() {
    let caps = Caps::of(&LIMITS, &budgets(), &WasmTuning::designed());
    let mut state = State::new(&caps);
    let root = state.nodes.root().rep();
    let label = HostNode::new(
        &mut state,
        Kind::Label,
        Resource::new_borrow(root),
        0,
    )
    .unwrap()
    .rep();
    let mut set = || {
        HostNode::set(
            &mut state,
            Resource::new_borrow(label),
            Property::Text("a".repeat(600)),
            None,
        )
    };
    assert!(set().is_ok());
    assert_eq!(Fault::of(set().unwrap_err()), Fault::Bytes);
}

#[test]
fn hostile_plugins_are_held_to_their_budgets() {
    let (mut instance, caps) =
        start(&declaring("(i32.const 0)"), &LIMITS, &budgets());
    assert_eq!(instance.declare(&caps).0, Ok(Vec::new()));

    let endless = "(loop $again (br $again)) (i32.const 0)";
    let cases: [(&str, fn(&mut Budgets), Fault); 4] = [
        // A list twice as long as a call may hand over.
        (
            "(i32.store (i32.const 4) (i32.const 2048)) (i32.const \
             0)",
            |_| {},
            Fault::Bytes,
        ),
        // A line of the log as long.
        (
            "(call $log (i32.const 0) (i32.const 0) (i32.const \
             2048))
             (i32.const 0)",
            |_| {},
            Fault::Bytes,
        ),
        // Two memories, each within the budget and together past it.
        (
            "(drop (memory.grow $more (i32.const 3))) (i32.const 0)",
            |_| {},
            Fault::Memory,
        ),
        (endless, |budgets| budgets.call_time = 1.0, Fault::Time),
    ];
    for (declare, adjust, fault) in cases {
        let mut budgets = budgets();
        adjust(&mut budgets);
        let (mut instance, caps) =
            start(&declaring(declare), &LIMITS, &budgets);
        assert_eq!(
            instance.declare(&caps).0,
            Err(fault),
            "{declare}"
        );
    }
}

#[test]
fn the_largest_limits_and_budgets_are_taken() {
    let limits = PluginLimits { memory: u64::MAX };
    let budgets = Budgets {
        call_time: f32::MAX,
        log_lines_per_second: u32::MAX,
        bytes_per_call: u64::MAX,
        node_changes_per_call: usize::MAX,
        ..budgets()
    };
    let (mut instance, caps) =
        start(&declaring("(i32.const 0)"), &limits, &budgets);
    assert_eq!(instance.declare(&caps).0, Ok(Vec::new()));
}

#[test]
fn a_plugin_has_the_system_and_its_prints_are_cut() {
    let imports = r#"(module
  (import "wasi:clocks/monotonic-clock@0.2.0" "now" (func $now (result i64)))
  (import "wasi:clocks/monotonic-clock@0.2.0" "subscribe-duration" (func $sleep (param i64) (result i32)))
  (import "wasi:io/poll@0.2.0" "[method]pollable.block" (func $block (param i32)))
  (import "wasi:random/random@0.2.0" "get-random-u64" (func $random (result i64)))
  (import "wasi:cli/stdout@0.2.0" "get-stdout" (func $stdout (result i32)))
  (import "wasi:io/streams@0.2.0" "[method]output-stream.blocking-write-and-flush" (func $write (param i32 i32 i32 i32)))"#;
    // Declaring, it reads the clock and a random number, then writes
    // a line of 2,000 bytes to stdout 200 times.
    let declare = "(local $out i32) (local $left i32)
        (drop (call $now))
        (drop (call $random))
        (memory.fill (i32.const 1024) (i32.const 97) (i32.const \
                   1999))
        (i32.store8 (i32.const 3023) (i32.const 10))
        (local.set $out (call $stdout))
        (local.set $left (i32.const 200))
        (loop $again
          (call $write (local.get $out) (i32.const 1024)
            (i32.const 2000) (i32.const 512))
          (local.set $left (i32.sub (local.get $left) (i32.const 1)))
          (br_if $again (local.get $left)))
        (i32.const 0)";
    // Running, it sleeps for a thousand seconds.
    let run = "(call $block (call $sleep (i64.const 1000000000000)))";
    let text = module(declare, run).replacen("(module", imports, 1);
    let (mut instance, caps) = start(
        &component(&text, true),
        &LIMITS,
        &Budgets {
            call_time: 20.0,
            ..Budgets::designed()
        },
    );

    assert_eq!(instance.declare(&caps).0, Ok(Vec::new()));
    let logs = instance.take_logs();
    assert_eq!(logs.len(), caps.lines);
    assert!(logs.iter().all(|(level, text)| {
        *level == LogLevel::Info && text.len() == caps.line_bytes
    }));

    // The sleep ends with the call's time.
    let started = Instant::now();
    let _ = instance.run(&caps, "p");
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_plugin_over_its_budgets_waits_then_is_stopped() {
    // It is asked to run each frame, past its calls of a second.
    let root = home("over");
    install(
        &root,
        &format!(
            "id = \"test.over\"\nname = \"Over\"\nversion = \
             \"0.1.0\"\napi = \"{API_VERSION}\"\n"
        ),
        &component(&module("(i32.const 32)", ""), false),
    );
    let mut app = engine(&root);
    app.insert_resource(Budgets {
        strikes: 1,
        calls_per_second: 2,
        ..Budgets::designed()
    });
    let record = Record {
        allowed: true,
        version: "0.1.0".into(),
        bundled: false,
        answers: Vec::new(),
    };
    let id = PluginId::try_from("test.over").unwrap();
    app.world_mut()
        .resource_mut::<Grants>()
        .set(&id, record)
        .unwrap();
    let stopped = (0..500).find_map(|_| {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
        let world = app.world_mut();
        for mut inbox in world.query::<&mut Inbox>().iter_mut(world) {
            inbox.0.push_back(Work::Run("p".into()));
        }
        world.query::<&Stopped>().iter(world).next().cloned()
    });
    let Some(Stopped(why)) = stopped else {
        panic!("the plugin was never stopped");
    };
    assert_eq!(why, Fault::Over(1).to_string());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_reloaded_plugin_keeps_its_state() {
    let budgets = budgets();
    let tuning = WasmTuning {
        plugins: 1,
        ..WasmTuning::designed()
    };
    let host = Host::new(&tuning).expect("the host starts");
    let plugin = declaring("(i32.const 0)");
    let linked = link(&host.0, &plugin).expect("the plugin links");
    let caps = Caps::of(&LIMITS, &budgets, &tuning);
    let mut old = Instance::new(&host.0, &linked, &caps)
        .expect("the plugin starts");
    for _ in 0..3 {
        assert_eq!(old.run(&caps, "count").0, Ok(()));
    }
    let mut spending = Spending::new(Duration::ZERO);
    let (mut fresh, declared) = old
        .reload(&host.0, &linked, &caps, &mut spending)
        .expect("it reloads");
    assert_eq!(declared, Vec::new());
    assert_eq!(fresh.save(&caps).0, Ok(3_u32.to_le_bytes().to_vec()));
}

#[test]
fn a_bundled_plugin_is_let_in_unasked_and_stays() {
    let root = home("bundled");
    let bundle = root.join("bundle");
    let folder = bundle.join("test.bundled");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join(MANIFEST),
        format!(
            "id = \"test.bundled\"\nname = \"Bundled\"\nversion = \
             \"0.1.0\"\napi = \
             \"{API_VERSION}\"\n\n[[asks]]\ncapability = \
             \"net.fetch\"\ntarget = \"example.org\"\nreason = \"To \
             fetch\"\n"
        ),
    )
    .unwrap();
    fs::write(
        folder.join(WASM),
        component(&module("(i32.const 32)", ""), false),
    )
    .unwrap();
    let mut app = engine(&root);
    app.insert_resource(Bundle::at(&bundle));
    for _ in 0..3 {
        app.update();
    }
    let id = PluginId::try_from("test.bundled").unwrap();
    let world = app.world_mut();
    assert_eq!(world.query::<&Petition>().iter(world).count(), 0);
    let plugin = world
        .query_filtered::<Entity, With<Admitted>>()
        .single(world)
        .expect("it is let in");
    let record = world.resource::<Grants>().record(&id).cloned();
    let record = record.expect("it is recorded");
    assert!(record.bundled);
    assert_eq!(
        record.answer(Capability::NetFetch, "example.org"),
        Some(Answer::Allow)
    );

    world.trigger(Uninstall { entity: plugin });
    app.update();
    assert!(app.world().get_entity(plugin).is_ok());
    assert!(folder.join(WASM).is_file());
    assert!(app.world().resource::<Grants>().record(&id).is_some());
    let _ = fs::remove_dir_all(&root);
}

/// How a folder to add a plugin from is made.
#[derive(Clone, Copy, Debug)]
enum Made {
    Built,
    /// Its `plugin.wasm` a link to one elsewhere.
    Linked,
    /// Its `plugin.wasm` a named pipe.
    Special,
    Oversized,
    InHome,
    /// The source of a plugin, not built.
    Source,
    /// Asking for keys.
    Never,
}

#[test]
fn only_the_folder_of_a_built_plugin_is_added() {
    let root = home("adding");
    let outside = home("adding_from");
    let plugins = root.join(FOLDER);
    fs::create_dir_all(&plugins).unwrap();
    let tuning = WasmTuning {
        wasm_bytes: 64,
        ..WasmTuning::designed()
    };
    let into = plugins.join("test.adding");
    let mut cases = vec![
        (Made::Built, ""),
        (
            Made::Oversized,
            "plugin.wasm has 65 bytes, and the most is 64",
        ),
        (Made::InHome, "the folder is in the engine's home"),
        (Made::Source, "it has no plugin.wasm"),
        (Made::Never, "keys.read, and reading keys is never granted"),
    ];
    if cfg!(unix) {
        cases.push((Made::Linked, "plugin.wasm is a link"));
        cases
            .push((Made::Special, "plugin.wasm is not a plain file"));
    }
    for (made, refusal) in cases {
        let folder = match made {
            Made::InHome => plugins.join("inside"),
            _ => outside.join(format!("{made:?}")),
        };
        fs::create_dir_all(&folder).unwrap();
        let asks = match made {
            Made::Never => {
                "[[asks]]\ncapability = \"keys.read\"\ntarget = \
                 \"all\"\nreason = \"To sign\"\n"
            }
            _ => "",
        };
        fs::write(
            folder.join(MANIFEST),
            format!(
                "id = \"test.adding\"\nname = \"Adding\"\nversion = \
                 \"0.1.0\"\napi = \"{API_VERSION}\"\n{asks}"
            ),
        )
        .unwrap();
        let wasm = folder.join(WASM);
        match made {
            Made::Linked => {
                fs::write(outside.join("elsewhere.wasm"), [0; 8])
                    .unwrap();
                #[cfg(unix)]
                std::os::unix::fs::symlink(
                    outside.join("elsewhere.wasm"),
                    &wasm,
                )
                .unwrap();
            }
            Made::Special => {
                let made = Command::new("mkfifo").arg(&wasm).status();
                assert!(made.unwrap().success());
            }
            Made::Oversized => fs::write(&wasm, [0; 65]).unwrap(),
            Made::Source => {
                fs::write(folder.join("Cargo.toml"), "").unwrap()
            }
            _ => fs::write(&wasm, [0; 64]).unwrap(),
        }
        let added =
            check(&folder, &root, &tuning).and_then(|checked| {
                checked.place(&plugins, &into, &tuning)
            });
        match added {
            Ok(manifest) => {
                assert!(refusal.is_empty(), "{made:?} was added");
                assert_eq!(manifest.id.as_str(), "test.adding");
                assert_eq!(
                    fs::read(into.join(WASM)).unwrap(),
                    [0; 64]
                );
                fs::remove_dir_all(&into).unwrap();
            }
            Err(why) => assert!(
                why.contains(refusal) && !refusal.is_empty(),
                "{made:?}: {why}"
            ),
        }
        // Nothing is left behind in the plugins folder.
        let left = fs::read_dir(&plugins)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name != "inside")
            .collect::<Vec<_>>();
        assert!(left.is_empty(), "{made:?} left {left:?}");
    }
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&outside);
}

#[test]
fn a_scroll_under_frames_scrolls_within_its_panel() {
    let mut app = laid_out();
    let world = app.world_mut();
    let plugin = world.spawn_empty().id();
    let surface = world
        .spawn((
            Surface {
                name: "p".into(),
                stopped: None,
            },
            ContributedBy(plugin),
        ))
        .id();
    mount::<EngineTheme>(
        world,
        column((crate::retained::view(surface),))
            .width(px(300.0))
            .height(px(300.0)),
    );
    app.update();
    let root = app.world().get::<UiRoot>(surface).unwrap().holder;
    // A bare frame, as a plugin's `keyed` makes, over a column of a
    // label and a scroll of 40 cards.
    let make = |node, kind, parent| NodeChange::Make {
        node,
        key: node,
        kind,
        parent,
    };
    let set = |node, value| NodeChange::Set {
        node,
        value,
        motion: None,
    };
    let mut changes = vec![
        make(1, Kind::Frame, 0),
        make(2, Kind::Frame, 1),
        make(3, Kind::Label, 2),
        set(3, Property::Text("Hamlets".into())),
        make(4, Kind::Scroll, 2),
    ];
    for card in 5..45 {
        changes.push(make(card, Kind::Frame, 4));
        changes.push(set(card, Property::Fill(Fill::Card)));
    }
    app.world_mut()
        .build_nodes(plugin, Some((0, root)), changes);
    for _ in 0..4 {
        app.update();
    }

    let world = app.world_mut();
    let area = world
        .query::<(Entity, &NodeOf)>()
        .iter(world)
        .find_map(|(node, of)| (of.key == 4).then_some(node))
        .expect("the scroll is built");
    let height = |node| {
        let computed = world.get::<ComputedNode>(node).unwrap();
        (computed.size() * computed.inverse_scale_factor()).y
    };
    let scroll = world.get::<Children>(area).unwrap()[0];
    // The column of the cards, beside the scroll's bars.
    let rows = world
        .get::<Children>(scroll)
        .unwrap()
        .iter()
        .copied()
        .find(|&child| world.get::<Children>(child).is_some())
        .unwrap();
    assert!(height(area) > 0.0);
    assert!(height(area) < 300.0, "{}", height(area));
    assert!(height(rows) > height(area));
}

/// An app laying its nodes out in a window 1000 pixels square, with
/// the shell's views and the marks of what holds a scroll.
fn laid_out() -> App {
    let mut app = App::new();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        Duration::from_millis(10),
    ))
    .add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        AssetPlugin::default(),
        TransformPlugin,
        InputPlugin,
        PickingPlugin,
        InteractionPlugin,
        hamlet_engine_shell::views_plugin,
    ))
    .add_systems(Update, find_scrolls)
    .init_asset::<Image>()
    .init_resource::<TextPipeline>()
    .init_resource::<FontCx>()
    .init_resource::<ScaleCx>()
    .add_plugins(UiPlugin)
    // What the test leaves out, such as fonts, skips its run.
    .set_error_handler(ignore);
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::splat(1000),
                    scale_factor: 1.0,
                }),
                ..Default::default()
            },
            ..Default::default()
        },
    ));
    app
}

/// A home of its own for the test `name`, empty.
fn home(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "hamlet_engine_test_{name}_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    root
}

/// Installs the plugin of `manifest` and `wasm` in the home `root`.
fn install(root: &Path, manifest: &str, wasm: &[u8]) {
    let folder = root.join(FOLDER).join("plugin");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join(MANIFEST), manifest).unwrap();
    fs::write(folder.join(WASM), wasm).unwrap();
}

/// The engine without a window, on the home `root`.
fn engine(root: &Path) -> App {
    let mut app = App::new();
    app.insert_resource(Home::at(root)).add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        hamlet_engine_core::plugin,
        hamlet_engine_plugins::plugin,
        hamlet_engine_grants::plugin,
        crate::plugin,
    ));
    app
}
