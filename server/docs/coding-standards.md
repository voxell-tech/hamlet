# Server coding standards

Use strict TypeScript, Node 24, npm, Biome and Vitest with the settings
checked into `server/`. Use the Node version in
[`.nvmrc`](../.nvmrc); the root [`AGENTS.md`](../../AGENTS.md) gives the
checks and safe running rules.

## Modules and folders

A concept has one entry, `src/<name>.ts`, and implementation files in
`src/<name>/` when needed. Entries use named re-exports, never `index.ts`
or `export *`. Code outside the folder imports only its entry. Code
inside imports siblings and other concepts' entries, never its own entry
as a value. Type-only imports of its entry are allowed. Tests may import
internals to exercise an internal seam. `test/` stays flat.

Shared dependency leaves are an explicit exception: anyone may import the
following paths directly. They keep clocks, records, rules and low-level
operations separate from entries that compose parts.

| Concept | Dependency leaves under `src/` |
|---|---|
| chats | `chats/answers.ts`, `chats/instructions.ts`, `chats/notice.ts`, `chats/tasks.ts`, `chats/turn.ts` |
| hand | `hand/manifests.ts` |
| home | `home/abort.ts`, `home/clock.ts`, `home/credentials.ts`, `home/layout.ts` |
| house | `house/skill-paper.ts`, `house/skills.ts` |
| library | `library/shelf.ts` |
| limits | `limits/rules.ts` |
| models | `models/endpoints.ts` |
| oversight | `oversight/asked.ts`, `oversight/waits.ts`, `oversight/words.ts` |
| realm | `realm/access.ts`, `realm/places.ts`, `realm/roads.ts` |
| sandbox | `sandbox/policy.ts` |
| spend | `spend/ledger.ts`, `spend/money.ts`, `spend/usage.ts` |
| state | `state/activity.ts` |

These leaves may value-import only each other, the shared entries
`change.ts`, `commands.ts`, `connection.ts`, `protocol.ts`, `record.ts`,
`result.ts`, `sandbox.ts`, `scrub.ts`, `settings.ts`, and the legacy shared
conversation core: `chats/conversations.ts`, `chats/errands.ts`,
`chats/configure.ts`, `chats/documents.ts`. That core is also directly importable
and follows the same dependency rule. Type-only imports may reach the
owning entries. Only `cli/serving.ts` value-imports the composition root
`home.ts`; home helpers are imported from their leaves.

`test/structure.test.ts` checks these paths and the value-import graph,
including named re-exports, side-effect imports and literal dynamic
imports. Only its two explicitly listed legacy cycles are allowed:
realm founding/votes and departure/merge.
Files in a listed cycle may import each other directly, keeping the cycle
off their entries. Adding a member or joining cycles fails the check.

## Check values according to their source

- External input is untrusted: client protocol messages, files from a
  house or the home, environment variables, model tool arguments and
  sandbox helper replies. Parse it once at the receiving boundary with
  the TypeBox schema that owns its shape. JSON parsing and a library's
  generic type parameter do not prove that shape.
- Use TypeBox's `Value` functions, such as `Value.Check` and `Value.Errors`
  from `typebox/value`, to validate and explain rejected input. A compiled
  TypeBox validator serves the same purpose for a reused schema. Turn
  invalid input into a typed failure at that boundary.
- Pass an already typed value directly to code that accepts that type.
  Do not serialize and parse it again, or repeat structural guards inside
  the server.
- A newly computed value or a patch can introduce a new constraint.
  Establish it through the one constructor or refinement that owns the
  constraint. For example, normalizing an agent name with `tidyName` does
  not establish C10's length, alphabet and reserved-name rules; the name
  checker owns those rules.

Keep schemas and reusable validators beside their owner. Expose a parser
named for the domain operation, accepting the representation callers
actually have, rather than leaking a validation library's options.
Internal operations receive parsed values, not raw protocol objects or
storage representations. Encode only when an output boundary needs it.
An assertion is never a substitute for parsing.

## Preserve type evidence

Keep domain and library types through parameters, results and intermediate
values. Do not widen them to `unknown`, `object`, a loose record or a less
precise primitive to simplify an implementation. Reserve `unknown` for
input whose runtime shape really is unknown. Use the owning public type or
generic instead of a local approximation; use `satisfies` or an explicit
return type to check a new value without discarding its inferred detail.
Types and `satisfies` alone do not establish computed runtime constraints.

No `any`, non-null assertions (`!`), or assertion chains such as
`as unknown as T` or `as any as T`. Narrow absent values before use.
Do not erase a type and cast it back, even across separate statements.
When a library's type is insufficient, prefer its supported API, a typed
upstream correction, a parsed adapter or leaving unsupported behavior out.

An assertion is allowed only at an unavoidable platform or library
boundary where runtime evidence already proves the type and TypeScript
cannot express that proof. Keep it to the smallest expression, without
prior widening. Immediately above it, include:

- `// SAFETY:` with the value's source, the invariant proving the type and
  the limitation in TypeScript or the library.
- `// Verified by:` naming the focused test that detects a broken invariant.
- The narrowest `biome-ignore` for the specific diagnostic, if one fires.

Missing proof or verification blocks review. A lint suppression is not
proof, and convenience does not justify an assertion.

## Generated contracts

[`src/protocol.ts`](../src/protocol.ts) defines protocol v0.
`server/protocol.schema.json` and `crates/hamlet_protocol/src/v0.rs` are
generated files; never edit them by hand. Change the TypeBox contract,
run `npm run schema` in `server/`, then run
`cargo run -p hamlet_protocol_gen` at the repository root. Read both
generated diffs after every protocol edit.

Protocol v0 may gain optional additions only. Preserve existing fields,
meanings and accepted messages so older clients and recordings still work.
Document caller-relevant units, identity and ordering at the contract.
Examples must use safe Hamlet values and agree with the schema.

## Absent values

Use `T | null` for an absent value in a complete domain record. This is the
convention of pi-durable documents such as `Work.folder`,
`AgentRecord.model` and `With.chief`, and the spine's `Turn.task`, `chief`
and `from`. Many wire fields use the same representation, including
`Hamlet.folder` and `Transcript.earlier`. Do not add a second absence form
to the same domain field.

Use optional properties (`?:`) for options and overrides, for patches
where omission means leave unchanged, and for optional protocol additions.
For example, `HomeOptions.now` may be omitted, `ChatRef.past` selects an
earlier conversation when supplied, and omitting an edit's model differs
from clearing a nullable value. With `exactOptionalPropertyTypes`, omit
an optional property rather than assigning `undefined` to it.

Lookups and library APIs, such as `named` and Harness snapshots, may return
`undefined`; local lifecycle handles may also use it. Narrow it where used
and convert to the owning record's `null` when constructing that record.
Convert omitted or nullable external fields at their boundary according
to their meaning. Keep the protocol's declared distinctions intact.

## Errors

Return expected failures as `Result<T, E>` values. The error type `E` is a
discriminated union with a literal `kind` for each failure the caller must
handle. Async operations return `Promise<Result<T, E>>`; an expected
failure must not escape as a rejected promise.

```ts
type Result<T, E> =
    | { ok: true; value: T }
    | { ok: false; error: E };
```

Handle success and each error kind explicitly, using an exhaustive
`switch` and a `never` check. Throw only for a defect, such as a broken
invariant or a bug, or a "not built yet" stub. Translate third-party
exceptions into typed failures at their boundary when they describe an
expected outcome; do not disguise defects as routine failures.
Read [`errors.md`](errors.md) for messages, context, scrubbing and recovery.

## Wiring and control flow

Choose dependencies where the server starts, at the composition root,
and pass them into the capabilities that need them. `openHome` wires the
Harness, models, storage and clock; tests supply their implementations
through the same interfaces. Keep resource ownership and closing explicit.
Do not hide mutable state, dependency lookup or live resources at module
scope.

Write dependent asynchronous work with `async`/`await`, named intermediate
values and ordinary branches. Keep retry, timeout and logging policy
separate from domain decisions. Prefer a direct transformation for one
simple mapping rather than adding orchestration around it.

## Tests

Test Hamlet's behavior, not TypeBox's implementation. Do not turn a
schema's declared limits or union members into a table of assertions.
Useful schema tests cover our normalization, transformations, round trips,
compatibility or a regression. Exercise rejected input through the public
boundary when the failure has a caller-visible consequence.

Test doubles implement the complete interface and its relevant behavior.
Use the faux model provider for model calls, real loopback WebSockets for
protocol tests and real Harness homes in scratch folders for storage and
recovery. Reuse `test/support.ts` helpers and close resources after tests.
No partial mocks, cast-shaped substitutes or weaker production behavior
for test convenience. Keep test homes and credentials isolated from the
chief's own home. Tests reach neither the internet nor a model server
(S14); loopback connections belong to the test.

## Doc comments

Every exported symbol has a `/** ... */` comment at its original
declaration. Every public method and property of an exported class has
one too. State the sharpest caller-relevant fact its signature cannot
show: a constraint, side effect, ownership rule or domain meaning.
Document fields whose meaning goes beyond their name and type.

For example, `agentKey` uses the agent's stable number:

```ts
/** A document-family key that survives a change of agent name. */
export function agentKey(agent: AgentRef): string {
    return `${agent.hamlet}:${agent.number}`;
}
```

A documented protocol schema constant also documents the immediately
following `export type X = Static<typeof X>` alias of the same name.

Document original declarations once; re-exports use that documentation.
Use explicit prose rather than `@inheritDoc`. Add tags only when they
explain something further. Describe typed failures as returned outcomes;
reserve `@throws` for defects and unbuilt stubs.

This exported-symbol requirement overrides the root
[comment convention's](../../docs/comment_convention.md) preference for
fewer comments in `server/`. Its other rules still apply: concise ASCII,
no history, no restating signatures or lists of callers. Comment internal
code where a non-obvious invariant or safety rule needs explanation.

## Names and words

Use [`CONTEXT.md`](../../CONTEXT.md): chief, agent, mayor, house, hamlet,
town, realm, home, road and letter have distinct meanings. Name a
capability for its domain responsibility, with enough context to be useful
at an import site. Messages use the audience's words, not storage or
framework vocabulary.

Keep step numbers, slice ids, ticket names and planning words out of code,
comments and messages. Requirement IDs such as C10 and S14 refer to stable
rules in the spec, not a work plan.
