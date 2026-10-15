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

**Status:** early and moving fast.

## Running

Hamlet needs an OpenAI-compatible endpoint. The default is a local
[Ollama](https://ollama.com) with `qwen3.6:35b-a3b`, and Settings holds the
endpoints, their keys and the model.

```sh
git submodule update --init   # once after cloning, for the UI library
cargo run                     # the window
cargo run -- --help           # the other modes, flags and environment overrides
```

Tools run in a sandbox on macOS and Linux. Windows has none yet and needs
`--insecure`.

Hamlet keeps everything in `~/.hamlet`. Set `HAMLET_HOME` to a scratch
folder when trying things out, so that your real notes, skills and logs are
left alone.

In the window, drag the ground to slide it, drag with the right button to
turn, and scroll to zoom. Click a robot or a house to talk to its agent,
and right-click anything for what can be done to it.

## Working on the code

```sh
cargo build
cargo clippy --all-targets
cargo test
cargo +nightly fmt
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
