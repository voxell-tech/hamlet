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

**Hamlet** is an agent orchestrator. You lead a team of AI agents, and
watch them work as a hamlet: a small 3D world where each agent has a house
and a robot. Free and open-source forever.

<p align="center">
  <img src="brand/hamlet-window.png" alt="The Hamlet window: petitions for the chief on the left, a realm of four hamlets joined by roads across a river in the middle, and the list of hamlets with their agents and budgets on the right" width="860">
</p>

## Features

- **One lead, many agents**: You are the chief and talk to the mayor, the
  lead agent. It creates the agents the work needs, each with its own
  notes, skills and log.
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

**Status:** early and moving fast. The agents are being rebuilt as a
server on pi-durable that the window connects to. Agents, roads, letters,
votes, the shared library and several hamlets work through it, alongside
approvals, budgets, models and logins. The window reads the server's state
and can watch agents' conversations read-only. The clone and git proxy are
[next](docs/proposals/agent-server.md#order-of-work).
The picture above shows the earlier Rust runtime's realm.

## Running

Use Rust and the Node version in `server/.nvmrc`. From the repository root:

```sh
git submodule update --init vendor/fynix
npm ci --prefix server
HAMLET_HOME=/tmp/hamlet-try cargo run -p hamlet_cli -- dev
```

The Rust CLI builds the local window, starts a separate TypeScript server
on a free port, and opens the window. No `npm link` is needed.
The home is kept between runs; the window's files go in `<home>/window`.
The welcome offers API keys, Continue with ChatGPT, OpenRouter sign-in or
a local model endpoint on the server's machine. Credentials stay on the
server. Tools need Seatbelt on macOS or bubblewrap on Linux; the server
cannot start on Windows, and there is no `--insecure` mode.

Closing the window leaves `hamlet dev` serving; Ctrl-C stops it. Use
`cargo run -p hamlet_cli -- <command>` without installing, or install the
CLI once with `cargo install --path crates/hamlet_cli`. If you previously
linked the npm command, remove it with `npm unlink -g hamlet-server` so it
cannot shadow the Rust command. To keep a server separate from the window
after installing, use two terminals:

```sh
HAMLET_HOME=/tmp/hamlet-try hamlet server
# In the other terminal:
HAMLET_HOME=/tmp/hamlet-try HAMLET_VIEW=/tmp/hamlet-try/window hamlet window
```

`hamlet chat` is the terminal client. `hamlet --help` lists the commands.
`HAMLET_APP` selects an existing `hamlet-app` and skips `dev`'s build.
Other window commands prefer an app beside the CLI, then one on PATH.
The installed CLI still needs Node and the server package; it uses the
source checkout unless `HAMLET_SERVER_DIR` names another package directory.
`HAMLET_NODE` selects a Node executable. Release archives contain the window, not the server;
Hamlet.app still opens as Hamlet while its executable is `hamlet-app`.

The server's default home is `~/.hamlet`; use a scratch `HAMLET_HOME` when
trying things out. The window keeps its own size, layout and look in
`HAMLET_VIEW`, else the OS's config folder for Hamlet. When running it
outside `hamlet dev`, set `HAMLET_VIEW` to a scratch folder too.

In the window, drag the ground to slide it, drag with the right button to
turn, and scroll to zoom. Click a robot or a house to talk to its agent,
and right-click anything for what can be done to it.

## Working on the code

```sh
cargo +nightly fmt
cargo build --all-targets
cargo clippy --all-targets
cargo test
(cd server && npm run check && npm test)
```

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
