# A tour of the code

Where things are and where to change them. For what Hamlet is and how to
run it, read [`README.md`](../README.md) first. For the rules of working
here, [`AGENTS.md`](../AGENTS.md).

## The idea in one paragraph

Agents run on the TypeScript server in `server/`, on pi-durable, with their
conversations and records in the home's storage and their writable files
in houses. The Rust window is a client: `hamlet_client` fills Bevy entities
from protocol v0 over one authenticated WebSocket. The world and panels
read those entities and send the chief's deeds back to the server. The
server owns the rules and the truth; the window keeps only its own view
files, and closing it stops no agent.

## How a change travels

```mermaid
flowchart TD
    typed([You type a line])
    chat["hamlet_ui: chat"]
    outgoing["hamlet_client: messages, Outbox,<br/>connection"]
    server["server/: commands, agent turns,<br/>tools, storage and rules"]
    incoming["hamlet_client: receive,<br/>realm, waits and chats"]
    entities["Entities: Settlement, Citizen,<br/>Mayor, Busy, Waiting, Transcript ..."]
    society["Society: towns, agents on plots, roads"]
    drawn["Ground, houses, roads,<br/>robots, light, sound"]
    panels["Panels: hamlet_ui"]

    typed --> chat
    chat -- UserMessage --> outgoing
    outgoing -- "say" --> server
    server -- "state / change / turn / transcript" --> incoming
    incoming --> entities
    entities -- "hamlet_world: hamlet<br/>the bridge" --> society
    society -- "hamlet_world: generator" --> drawn
    entities --> panels
```

## The crates

All are under `crates/`. The `hamlet-app` binary and the examples are the
root package, `hamlet`. The window's crates are:

| Crate | What it is |
|---|---|
| `hamlet_protocol_gen` | Generates Rust protocol types from `server/protocol.schema.json`; run with `cargo run -p hamlet_protocol_gen` |
| `hamlet_protocol` | Checked-in protocol v0 types in `src/v0.rs`, with a test that catches schema drift |
| `hamlet_client` | The client core: the connection, Outbox, received realm, waits and watched chats. Bevy app/ECS, no rendering |
| `hamlet_desktop` | The window's machine: kept awake while a received agent is busy, file and link openers, temporary editor files, the folder dialog and the app's icon |
| `hamlet_assets` | The files under `assets/`, built into the app and loaded as Bevy assets |
| `hamlet_vfx` | The visual effects: what each is and how it plays |
| `hamlet_world` | The 3D world, drawn from the client's entities |
| `hamlet_ui` | The window: panels, menus, chat and pointer around the world |
| `hamlet_testing` | Scratch folders and protocol-value builders for offline tests |

The Rust runtime crates remain compiled and tested until step 7. No window
crate depends on them; `tests/window_is_a_client.rs` guards that boundary.
They are porting references, described in [`core.md`](core.md):

| Crate | Retained runtime part |
|---|---|
| `hamlet_sandbox` | Per-platform process sandboxes |
| `hamlet_model` | Model clients, settings, catalogue and session logs |
| `hamlet_home` | The Rust home's layout, charter, prompt, limits and starter skills |
| `hamlet_rules` | Records and rules over them, without Bevy |
| `hamlet_core` | Runtime plugins: agents, sessions, tools and oversight |

The active runtime is `server/`, its own Node package. `src/protocol.ts`
defines the wire shapes; `src/state.ts` and `src/turns.ts` publish state and
turns; `src/server.ts` routes requests; `src/cli.ts` implements the `hamlet`
command. Recorded faux sessions in `server/test/frames/` are checked by the
server tests and replayed by the Rust client tests.

Inside a crate, a module with several subjects is a file and a folder of
the same name, and each file opens with a line saying what it holds. Read
`lib.rs` for the list.

`hamlet_world` has one folder for each thing in the world: `generator/`
(land, towns, houses, roads and traffic from a seed), `keeper/` (the
robots), `chief/` (the wizard and its Hearth), `hand/` (what the chief does
by hand), `pick/` (pointing, selecting and the camera) and `sound/`.
`hamlet_ui` has one file or folder for each panel, and `theme.rs`,
`looks.rs`, `feel/` and `icons.rs` for how they look.

Names to keep apart:

- `hamlet_client/src/realm.rs` holds the received hamlets and agents;
  `chats.rs` holds the watched conversations.
- `server/src/hamlet.ts` holds the server's hamlet record.
  `hamlet_world/src/hamlet.rs` is the bridge that puts received hamlets
  into the world.
- `chief.rs` is the wizard in `hamlet_world` and the chief's panel in
  `hamlet_ui`; `Chief` in `hamlet_client` is the server's chief name.
- `hand.rs` is the hand in `hamlet_world`; `orders.rs` sends its deeds to
  the server.

## Where do I change...

Rust crate paths are under `crates/`; `server/` paths are from the
repository root.

| To change | Go to |
|---|---|
| What an agent is told every turn | `server/src/prompt.ts`, `parts.ts`; the charter is `server/assets/charter.md` |
| The tools an agent has | `server/src/tools.ts`, `tools/`; the mayor's tools in `server/src/mayor.ts` |
| A chief's slash command | `server/src/commands.ts`; the server advertises supported deeds in `state.deeds` |
| What needs the chief's approval | `server/src/gate.ts`, `danger.ts`, `waits.ts` |
| What an agent's commands may touch | `server/src/sandbox/`, `environment.ts`; [`sandbox.md`](sandbox.md) describes the reach policy |
| The folder a hamlet works on | `server/src/work.ts`, `environment.ts`; the panel is `hamlet_ui/src/folder.rs` |
| The workbench and its clockwork on a common | `hamlet_world/src/works.rs` |
| The library, agent lifecycle or the realm | [The society and realm steps](proposals/agent-server.md#order-of-work); retained Rust rules are the porting reference |
| Spending and limits | `server/src/budget.ts`, `ledger.ts`, `report.ts`, `limits.ts`, `limits/edit.ts` |
| Models, their prices and who may run them | `server/src/catalogue.ts`, `models.ts`, `providers.ts` |
| The model harness | `server/src/home.ts`, `turn.ts`; pi-durable owns the model calls |
| Signing in with ChatGPT to run models on the chief's plan | `server/src/logins.ts`, `credentials.ts`, `providers.ts` |
| A starter skill | `server/assets/starters/<name>/SKILL.md` |
| The protocol or its Rust types | `server/src/protocol.ts`; export with `npm run schema`, then `cargo run -p hamlet_protocol_gen` |
| Connection, reconnection or received components | `hamlet_client/src/connection.rs`, `realm.rs`, `waits.rs`, `chats.rs` |
| A panel | Its file in `hamlet_ui/src/` |
| The right-click menu | `hamlet_ui/src/world_menu/items.rs` |
| A colour in the panels | `assets/theme.toml`, its fields in `hamlet_ui/src/theme.rs` |
| How a control looks or feels | `hamlet_ui/src/looks.rs`, `feel/`; [`ui-warmth.md`](ui-warmth.md) |
| An icon | `hamlet_ui/src/icons.rs`; [`icons.md`](icons.md) |
| A colour in the world | `assets/palette.toml` |
| A size, speed or timing | `assets/world.toml`, `vfx.toml`, `ui.toml`. What each value means is beside its field in the code |
| The height something stands at | `LandData::surface`: the ground as it is drawn, never the smooth `height` |
| How a house looks | `hamlet_world/src/generator/house.rs` |
| How a robot moves | `hamlet_world/src/keeper/acts/`; [`robots.md`](robots.md) |
| An effect | `hamlet_vfx/src/`; when it is set off, `hamlet_world/src/generator/magic.rs` |
| A sound or the music | `hamlet_world/src/sound/`; [`sound.md`](sound.md) |
| The camera | `hamlet_world/src/pick/camera.rs` |
| The wizard or the Hearth | `hamlet_world/src/chief/`; [`chief.md`](chief.md) |
| The window's saved layout, size and look | `hamlet_ui/src/remember.rs`, `window.rs`, `hamlet_world/src/chief/look.rs`; files under `HAMLET_VIEW` |
| The server's settings or chief documents | `server/src/settings.ts`, `reads.ts`, `edits.ts`; the panels send protocol requests |

## Tuning while the app runs

The files under `assets/` are built into the app. With the `hot-reload`
feature they are also watched: save a change and the running app takes it.

```sh
cargo build --bin hamlet-app --features hot-reload
HAMLET_HOME=/tmp/hamlet-try HAMLET_APP="$PWD/target/debug/hamlet-app" hamlet dev
```

Three things do not follow a saved change: the `[generator]` section of
`world.toml`, since the land keeps the settings it was built from; the
colour of a mesh that is already built; and the charter, which is owned by
the server and edited through its protocol.

A file that cannot be read is reported and the last good values stay.

## Seeing your work without a window

These draw to files and open nothing:

| Command | Shows |
|---|---|
| `cargo run --example ui_gallery -- --scene <name> --out <dir>` | One panel scene. `examples/ui_gallery/main.rs` lists the scenes |
| `cargo run --release --example far_lands -- --out <dir>` | The land, many towns, a town being moved; `--bench` times it |
| `cargo run --example chief_sheet -- --out <dir>` | The wizard, its looks, a Wickstep, the library |
| `cargo run --example cursor_sheet` | Every cursor |
| `cargo run --release --example sound_sheet` | Every sound and the music, as WAV files |

`style_test` (one fixed scene and the robots' motions) and `town_gen` (the
generated world, grown from the keyboard) open a window.

Run the pair against a scratch home, never your own:

```sh
HAMLET_HOME=/tmp/hamlet-try hamlet dev
```

`npm link` in `server/` installs `hamlet`; use its `.nvmrc` Node version.
An old Cargo-installed `hamlet` can shadow it on PATH. `hamlet dev` sets
`HAMLET_VIEW=<home>/window`; when opening a window yourself, set a scratch
`HAMLET_VIEW` too. A server can outlive any number of windows; Ctrl-C in
the `hamlet dev` terminal stops the server.
