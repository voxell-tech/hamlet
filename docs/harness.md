# The agent's host

Every agent is an actor of the Node host, `hosts/node`, which the server
starts as its child and talks to over the child's pipe (A2, A3 in
[the actor runtime](spec/actor-runtime.md)). This page says what is in the
Node host and how an agent's conversations run there. Its files are listed
in [the tour](tour.md#the-node-host).

## What the Node host holds

| Path in `hosts/node/src/` | What it is |
|---|---|
| `main.ts`, `entry.ts` | The entry: it takes the link from the Rust host that started it and serves its kinds until that host's goodbye. `--faux <script>` has the agents' models answer from a script, `--sign-in` lists two providers on loopback to sign in to, and `--kind` serves the demo's kinds |
| `actor/` | The actor runtime's TypeScript side, under Apache-2.0 alone (A17) |
| `pi/` | pi-durable, the wrapper that keeps an agent's conversations, turns and timers in the actor's own file, under Apache-2.0 alone |
| `agent/` | The agent actor: its chats, turns, tools, prompt, filesystem and models |
| `demo.ts` | The demo's kinds, which `cargo run -p hamlet_host --example demo -- --link` runs |

The native module `@hamlet/actor-node`, built from `crates/hamlet_actor_node`,
gives the Node host the link, each actor's SQLite file and the agent's
filesystem, all in Rust. Install the
package with `npm ci` in `hosts/node`, with the Node version in
`hosts/node/.nvmrc`.

## How an agent's conversations run

An agent actor is per agent: its key is the agent's number, and its file,
`actors/agent/<n>.sqlite`, holds all its conversations
(`agent/sessions.ts`). A chief's chat is a run of sessions, each one
pi-durable conversation, one of them current. A peer's letters and its
questions each have a chat of their own (C22). A worker and an errand are
agents of their own.

A turn runs its tool calls one at a time (`toolExecution: "sequential"` in
`agent/actor.ts`). A chief's line that arrives during a running turn steers
it: the turn takes the line at its next model call (C23). A tool call that
has to wait does not hold the turn. It ends with words that say what it
waits for, and what it waited for comes to the agent as a new input:

| Call | Its words |
|---|---|
| A shell command past 5 s (C16) | The command's id. Its end arrives as a new input |
| A call that waits for a chief | "This call waits for a chief's answer." |
| `ask` | "Waiting for X's answer." |
| `start_worker` | The worker's answer comes as a reply |
| `ask_librarian` | "The librarian is looking." |

Each tool call and its result are kept in the agent's file, so a call cut
off by a stop runs again under the same id at the next start (A16).

## Request ids

A chief's line is admitted under its `say` request's id, so a line sent
again is admitted once. A retry is admitted under `retry:<message id>`
(`agent/commands.ts`), and a reflection under
`reflection:<conversation>:<k>` (`agent/reflect.ts`).

## Timers and images

pi-durable keeps each timer's sleep in the agent's file, and the host keeps
one alarm for the earliest (`agent/timers.ts`). The images saved with a
chief's line are written under `inbox/images/` in the agent's filesystem
(`agent/parcels.ts`) and handed to the model as image content beside the
words.

## Tests

In `hosts/node`, `npm test` runs the agent's and pi-durable's tests on faux
models, `npm run test:native` the native module's, and `npm run test:link`
the link's against a Rust host. `cargo xtask node` runs them all, as the
checks in `AGENTS.md` list.
