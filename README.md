<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="brand/hamlet-logo-wordmark-transparent.png">
    <img src="brand/hamlet-logo-wordmark-transparent-light.png" alt="Hamlet" width="280">
  </picture>
</p>

# Hamlet

Humans and Agents Making Little Evolving Towns: AI agents that live as a
hamlet, in Rust.

<p align="center">
  <img src="brand/hamlet-window.png" alt="The Hamlet window: petitions for the chief on the left, a realm of four hamlets joined by roads across a river in the middle, and the list of hamlets with their agents and budgets on the right" width="860">
</p>

You are the chief. The agent you talk to is the mayor. It starts with two
tools (a sandboxed shell and a file editor), a notes file and a folder of
skills, and grows the rest itself. It can create other agents, each with a
house of its own (notes, skills and a log), and agents talk to each other
along roads the mayor lays. Agents can vote a mayor out, hamlets can sit side
by side, and the chief can overrule anything.

A desktop command center shows the hamlet as a generated 3D world, with a
robot and a house per agent, beside panels for what needs you, who is
selected and who lives there.

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
