//! The terminals of the gallery: an agent's commands and the chief's
//! shell in one hamlet, a shell that ended in another, and an
//! unbounded one.

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use hamlet_app::terminal::{PanelId, Panels, show};
use hamlet_client::terminals::{self, TerminalScreen, TerminalView};
use hamlet_protocol::client as protocol;
use serde_json::json;

use crate::scenes::Scene;

/// What the founder's commands printed.
const RUNS: &str = concat!(
    "\x1b[36m$ cargo test -p orchard\x1b[0m\r\n",
    "\x1b[1;32m   Compiling\x1b[0m orchard v0.3.1 \
     (/home/orchard)\r\n",
    "\x1b[1;32m    Finished\x1b[0m `test` profile [unoptimized + \
     debuginfo] target(s) in 4.21s\r\n",
    "\x1b[1;32m     Running\x1b[0m unittests src/lib.rs \
     (target/debug/deps/orchard-3f2a9c)\r\n",
    "\r\n",
    "running 6 tests\r\n",
    "test rows::sorts_by_ripeness ... \x1b[32mok\x1b[0m\r\n",
    "test rows::skips_the_fallen ... \x1b[32mok\x1b[0m\r\n",
    "test picking::fills_the_basket ... \x1b[32mok\x1b[0m\r\n",
    "test picking::leaves_the_green ... \x1b[32mok\x1b[0m\r\n",
    "test notes::reads_old_notes ... \x1b[33mignored\x1b[0m\r\n",
    "test notes::keeps_the_newest ... \x1b[1;31mFAILED\x1b[0m\r\n",
    "\r\n",
    "failures:\r\n",
    "\r\n",
    "---- notes::keeps_the_newest stdout ----\r\n",
    "thread 'notes::keeps_the_newest' panicked at \
     \x1b[4msrc/notes.rs:88:9\x1b[0m:\r\n",
    "assertion `left == right` failed\r\n",
    "  left: \x1b[31m3\x1b[0m\r\n",
    " right: \x1b[32m4\x1b[0m\r\n",
    "\r\n",
    "test result: \x1b[1;31mFAILED\x1b[0m. 4 passed; 1 failed; 1 \
     ignored; finished in 0.02s\r\n",
    "\x1b[2m[exit code 101]\x1b[0m\r\n",
    "\x1b[36m$ git status --short\x1b[0m\r\n",
    " \x1b[31mM\x1b[0m src/notes.rs\r\n",
    "\x1b[31m??\x1b[0m notes/today.md\r\n",
    "\x1b[2m[done]\x1b[0m\r\n",
    "\x1b[36m$ cargo test -p orchard notes\x1b[0m\r\n",
    "\x1b[1;32m   Compiling\x1b[0m orchard v0.3.1 \
     (/home/orchard)\r\n",
);

/// What the chief typed in the shell, and what it printed.
fn shell() -> String {
    let prompt =
        "\x1b[1;32mchief@home\x1b[0m:\x1b[1;34m~/orchard\x1b[0m$ ";
    let mut out = String::new();
    out.push_str(prompt);
    out.push_str("ls --color\r\n");
    out.push_str(concat!(
        "\x1b[1;34mnotes\x1b[0m        \x1b[1;34msrc\x1b[0m         ",
        "\x1b[1;32mharvest.sh\x1b[0m   \x1b[36mlatest\x1b[0m -> \
         notes/today.md\r\n",
        "Cargo.lock   Cargo.toml   README.md    ",
        "\x1b[1;31mbackup.tar.gz\x1b[0m\r\n",
    ));
    out.push_str(prompt);
    out.push_str("printf 'styles'\r\n");
    out.push_str(concat!(
        "\x1b[1mbold\x1b[0m \x1b[3mitalic\x1b[0m \
         \x1b[4munderline\x1b[0m ",
        "\x1b[7minverse\x1b[0m \x1b[2mdim\x1b[0m ",
        "\x1b[30;43m black on yellow \x1b[0m \x1b[97;45m white on \
         magenta \x1b[0m\r\n",
    ));
    for index in 0..16 {
        out.push_str(&format!("\x1b[48;5;{index}m   "));
    }
    out.push_str("\x1b[0m  ");
    for step in 0..24 {
        let index = 232 + step;
        out.push_str(&format!("\x1b[48;5;{index}m "));
    }
    out.push_str("\x1b[0m\r\n");
    for step in 0..72u32 {
        let red = 255 - step * 255 / 72;
        let green = step * 180 / 72 + 40;
        let blue = step * 255 / 72;
        out.push_str(&format!("\x1b[48;2;{red};{green};{blue}m "));
    }
    out.push_str("\x1b[0m\r\n");
    out.push_str(prompt);
    out.push_str("git log --oneline -3\r\n");
    out.push_str(concat!(
        "\x1b[33m4e1c2a7\x1b[0m (\x1b[1;36mHEAD -> \
         \x1b[1;32mmain\x1b[0m) Keep the newest notes first\r\n",
        "\x1b[33m9b03f11\x1b[0m Pick only the ripe rows\r\n",
        "\x1b[33m1a7d5e0\x1b[0m Start the orchard\r\n",
    ));
    out.push_str(prompt);
    out.push_str("cargo te");
    out
}

fn view(
    id: &str,
    name: &str,
    hamlet: Option<&str>,
    agent: Option<u64>,
    kind: &str,
    exit: Option<i64>,
) -> TerminalView {
    serde_json::from_value(json!({
        "agent": agent,
        "cols": 110, "rows": 30, "cwd": "/home/orchard", "exit": exit,
        "hamlet": hamlet, "id": id, "kind": kind, "name": name,
        "sandboxed": hamlet.is_some(), "title": ""
    }))
    .expect("a terminal")
}

/// Adds the hamlets, the agent and the terminals to `state`.
pub(super) fn state(state: &mut protocol::State, scene: Scene) {
    if !matches!(
        scene,
        Scene::Terminal
            | Scene::TerminalShell
            | Scene::TerminalPanels
    ) {
        return;
    }
    state.hamlets.push(hamlet_testing::realm::hamlet("mill", 3));
    state
        .agents
        .push(hamlet_testing::realm::agent(3, "mill", true));
    state.terminals = vec![
        view(
            "t1",
            "commands",
            Some("home"),
            Some(2),
            "commands",
            None,
        ),
        view("t2", "zsh", Some("home"), None, "shell", None),
        view(
            "t3",
            "commands",
            Some("mill"),
            Some(3),
            "commands",
            None,
        ),
        view("t4", "bash", Some("mill"), None, "shell", Some(0)),
        view("t5", "login", None, None, "shell", None),
    ];
    state.terminals[1].last = Some("git log --oneline -3".into());
}

/// Shows the scene's terminals, each in its panel, which are given
/// what they printed once the panels watch them.
pub(super) fn setup(world: &mut World, scene: Scene) {
    let shown = match scene {
        Scene::Terminal => {
            vec![(PanelId::first(), "t1", RUNS.to_owned())]
        }
        Scene::TerminalShell => {
            vec![(PanelId::first(), "t2", shell())]
        }
        Scene::TerminalPanels => vec![
            (PanelId::first(), "t1", RUNS.to_owned()),
            (PanelId("terminal-2".into()), "t2", shell()),
        ],
        _ => return,
    };
    let mut printed = Vec::new();
    for (panel, id, bytes) in shown {
        let Some(terminal) = terminals::find(world, id) else {
            continue;
        };
        show(world, &panel, terminal);
        printed.push((terminal, bytes));
    }
    if scene == Scene::TerminalPanels {
        world
            .resource_mut::<Panels>()
            .hide_list(&PanelId("terminal-2".into()));
    }
    world.insert_resource(Printed(printed));
}

/// What the scene's terminals printed, until their screens have it.
#[derive(Resource)]
pub(crate) struct Printed(Vec<(Entity, String)>);

/// Gives each watched terminal what it printed, as a server would.
pub(crate) fn print(world: &mut World) {
    let Some(Printed(mut left)) = world.remove_resource::<Printed>()
    else {
        return;
    };
    left.retain(|(terminal, printed)| {
        let Some(mut screen) =
            world.get_mut::<TerminalScreen>(*terminal)
        else {
            return true;
        };
        screen.process(printed.as_bytes());
        false
    });
    if !left.is_empty() {
        world.insert_resource(Printed(left));
    }
}
