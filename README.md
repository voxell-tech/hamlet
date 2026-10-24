<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="brand/hamlet-logo-wordmark-transparent.png">
    <img src="brand/hamlet-logo-wordmark-transparent-light.png" alt="Hamlet" width="280">
  </picture>
</p>

# Hamlet

[![License](https://img.shields.io/badge/license-MIT%2FApache-blue.svg)](https://github.com/voxell-tech/hamlet#license)
[![CI](https://github.com/voxell-tech/hamlet/workflows/CI/badge.svg)](https://github.com/voxell-tech/hamlet/actions)
[![Discord](https://img.shields.io/discord/442334985471655946.svg?label=&logo=discord&logoColor=ffffff&color=7389D8&labelColor=6A7EC2)](https://discord.gg/Mhnyp6VYEQ)

**Hamlet** is an agent orchestrator. You lead a hamlet of AI agents and
watch them work in a small 3D world where each agent has a house and a
robot. Free and open-source forever.

<p align="center">
  <img src="brand/hamlet-window.png" alt="The Hamlet window: petitions for the chief on the left, a realm of four hamlets joined by roads across a river in the middle, and the list of hamlets with their agents and budgets on the right" width="860">
</p>

## Features

- **One lead, many agents**: You are the chief and talk to the mayor, the
  agent that creates the others the work needs and lays the roads. Each
  agent has its own notes, skills and log.
- **Agents that grow**: An agent starts with a sandboxed shell, a file
  editor and a notes file, and writes its own skills from there.
- **Agents talk to each other**: Messages travel along roads the mayor
  lays between houses.
- **Point a hamlet at your code**: Give a hamlet a folder and its agents
  work in it, as a coding agent does.
- **You stay in charge**: In manual mode every edit and command waits for
  your approval. In auto mode the sandbox keeps writes inside the folder.
  Agents can vote a mayor out, and the chief can overrule anything.
- **Many hamlets at once**: Hamlets sit side by side, each with its own
  agents, budget and folder.
- **Any model**: Any OpenAI-compatible endpoint, a local
  [Ollama](https://ollama.com) by default.

**Status:** early and moving fast. The Rust `hamlet-server` owns the
agents, roads, letters, votes, library, approvals and budgets. Node processes
running the pi harness own conversations and model calls. The window is a
protocol client; closing it stops no agent. Tests use faux and loopback
providers, but do not establish exhaustive behavior or platform coverage.

## Running

Use Rust, git and Node **24.1.0**, as pinned in `harnesses/pi/.nvmrc`.
From the repository root, install the CLI, server and window, and start on your
home, `~/.hamlet`:

```sh
cargo xtask install
hamlet
```

`cargo xtask install` installs pi's packages and puts `hamlet`,
`hamlet-server` and `hamlet-app` in `~/.cargo/bin`. This source
installation uses the checkout's `harnesses/pi/`, so keep the checkout and
run `cargo xtask install` again after pulling. Linux/macOS release archives contain
all three binaries and a `pi/` package with production dependencies; they
need installed Node but no checkout or npm install. Windows has a separate
window-only archive for connecting to a remote server, without Node or Pi.
See [releases](docs/releases.md).
`cargo xtask build` builds without installing, `cargo xtask all` builds
every binary, including `hamlet-workspace`, `cargo xtask uninstall` removes
all three binaries, and `cargo xtask dev` tries the checkout on
a scratch home without installing:

```sh
HAMLET_HOME=/tmp/hamlet-try cargo xtask dev
```

`dev` builds the local server and window, starts the server on a free
port, and opens the window. No global npm command is needed.
The home is kept between runs; the window's files go in `<home>/window`.
Use an API key, a local model endpoint, or sign in with ChatGPT or OpenRouter.
Credentials stay on the server; subscription refreshes are saved there
before the harness continues. Local tools need Seatbelt on macOS or working
bubblewrap on Linux. Settings can select a remote workspace for tools and
houses; it takes effect on restart and never falls back to local execution.
There is no `--insecure` mode or Windows server. The window can connect to
another machine's server, including from Windows.

Closing the window leaves `hamlet dev` serving; Ctrl-C stops it. Use
`cargo run -p hamlet_cli -- <command>` without installing. To keep a server
separate from the window after installing, use two terminals:

```sh
HAMLET_HOME=/tmp/hamlet-try hamlet server
# In the other terminal:
HAMLET_HOME=/tmp/hamlet-try HAMLET_VIEW=/tmp/hamlet-try/window hamlet window
```

`hamlet chat` is the terminal client. `hamlet --help` lists the commands.
`HAMLET_SERVER_BIN` selects an existing server and `HAMLET_APP` an existing
window; each skips the corresponding `dev` build. Otherwise installed
commands look beside the CLI, then on PATH. `HAMLET_SERVER` still means a
remote WebSocket URL, not an executable. `HAMLET_PI_HARNESS` selects pi's
`src/main.ts`; the server otherwise prefers sibling `pi/src/main.ts`, then
the macOS bundle's `Contents/Resources/pi/src/main.ts`, then the build
checkout's harness. `HAMLET_NODE` selects a Node executable.
The macOS bundle launches the CLI, which starts or reuses the server and
opens its sibling window.

The server's default home is `~/.hamlet`; use a scratch `HAMLET_HOME` when
trying things out. The window keeps its own size, layout and look in
`HAMLET_VIEW`, else the OS's config folder for Hamlet. When running it
outside `hamlet dev`, set `HAMLET_VIEW` to a scratch folder too.

In the window, drag the ground to slide it, drag with the right button to
turn, and scroll to zoom. Click a robot or a house to talk to its agent,
and right-click anything for what can be done to it.

## Working on the code

`vendor/fynix` is a git submodule: clone with `--recurse-submodules`, or
run `git submodule update --init vendor/fynix` once. `cargo xtask`
fetches it itself.

```sh
npm ci --prefix harnesses/pi
cargo +$(cat rust-nightly) fmt
cargo build --all-targets
cargo clippy --all-targets
cargo test
(cd harnesses/pi && npm run check && npm test)
```

`rust-toolchain.toml` pins stable; `rust-nightly` names the dated nightly
that formats. `cargo xtask check` runs all of it, plus the acceptance and
harness suites, cargo deny, typos and the docs, as steps CI runs by name:
`format`, `clippy`, `deny`, `typos`, `test`, `acceptance`, `harness`, `doc`
and `node`. `deny` and `typos` need
`cargo install --locked cargo-deny typos-cli`. `cargo xtask help` lists
every task.

[`docs/tour.md`](docs/tour.md) is the map of the code,
[`docs/`](docs/README.md) lists every page, and [`AGENTS.md`](AGENTS.md)
has the rules for working here.

## Join the community!

You can join us on the [Voxell discord server](https://discord.gg/Mhnyp6VYEQ).

## License

`hamlet` is dual-licensed under either:

- MIT License ([LICENSE-MIT](LICENSE-MIT) or
  [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  [http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0))

This means you can select the license you prefer! This dual-licensing
approach is the de-facto standard in the Rust ecosystem and there are
[very good reasons](https://github.com/bevyengine/bevy/issues/2373) to
include both.
