# Server error design

An error is an answer to a chief, a client or an agent. It must support a
useful decision without requiring the receiver to understand the server's
implementation.

## Messages and recovery

- Name the failed operation and the most precise reason that is safe to
  share. A missing agent, a refused tool and an unavailable model need
  different explanations. Keep the opening specific enough to search in
  the source.
- Give a next action: correct the named input, choose an available agent
  or model, ask a chief for permission, inspect the result, or report the
  failure with its request id. If the cause is uncertain, say so and give
  a safe inspection or reporting step.
- State whether repeating the same operation is safe. A transient cause
  alone does not make a retry safe. Check replay guarantees and possible
  partial effects first (S3). Never recommend a retry that could send a
  second letter, repeat a command or create another agent.
- When completion is uncertain, direct the receiver to inspect and
  reconcile the relevant record or output before acting again. Distinguish
  carrying on an unanswered turn (C26) from repeating its original deed.
- Reassure only about facts the server can prove. A failed multi-part
  operation does not justify claiming that nothing changed.
- Use calm, plain language and the words of
  [`CONTEXT.md`](../../CONTEXT.md). Avoid blame, jokes, exaggerated alarm,
  vague fallback text and unnecessary apologies. Chiefs need the domain
  reason, not a provider exception or storage implementation detail.

For a refused write in manual mode, explain that the write needs a chief's
approval and how to request it. Do not suggest bypassing the gate or
repeating the write until it succeeds.

## Typed failures and context

Expected failures are returned in `Result<T, E>`, with a discriminated union
of literal error `kind`s as `E`. Callers must handle every kind; async
operations keep these failures in their returned Result. Throw only for
defects and "not built yet" stubs. See
[`coding-standards.md`](coding-standards.md#errors).

Each error variant carries:

- A stable `kind` that callers can branch on independently of wording.
- The operation that failed.
- Safe known identities, such as an agent's hamlet and number, a chat or
  a tool name. Include a request id when the failure belongs to a request.
- A classified reason and the safe facts needed to explain it. Invalid
  input includes the field and the constraint needed to correct it.
- Whether retrying the same operation is safe, and the recovery action or
  information needed to choose one. An uncertain outcome is not retry-safe.

Use variant-specific fields so unrelated details cannot be attached to a
kind. Do not use a generic details record or force callers to extract
context from prose. Internal diagnostics may include the failing
capability and a safe underlying cause. Keep the operation and request id
available for tracing without recording secrets.

## Failure families in the server

`src/result.ts` owns `Result`, `ok` and `err`. Families live with the
operation that establishes their facts; entries expose the public ones.
Paths below are under `src/`. Family renderers handle kinds exhaustively
with unreachable `never` checks; dispatch helpers delegate to them.

| Family | Definition | Renderer |
|---|---|---|
| `ChiefRefusal` (the failure arm of `ChiefResult`) | `realm/access.ts` | `chiefRefusalText` |
| `AddressFailure` (including chief access) | `realm/roads.ts` | `addressFailureText` |
| `ResolveFailure` (tool addresses) | `realm/roads.ts` | `resolveFailureText` |
| `RecordRefusal` (membership, roads, office, votes and realm deeds) | `record/refusals.ts` | `recordRefusalText`, with chief or agent audience |
| `DecisionHeld` (an open vote) | `record/office.ts` | `decisionText`; `record/tools.ts`'s `toolText` dispatches this family and `RecordRefusal` for agents |
| `ConversationFailure` | `chats/failure.ts` | `conversationFailureText` |
| `WatchFailure` (including address and conversation failures) | `transcript/failure.ts` | `watchFailureText` |
| `CommandFailure` | `commands.ts` | `commandFailureText` |
| `Malformed` | `protocol.ts` | The envelope uses its parser-selected `text` |
| `ConnectionFailure` (transport rejection or defect) | `connection.ts` | `connectionFailureText` |
| `AnswerFailure` | `gate.ts` | `answerFailureText` |
| `WorkFailure` (work-folder edits) | `environment/work.ts` | `workFailureText` |
| `NotesFailure` | `house/skills.ts` | `notesFailureText` |
| `TreeFailure` | `house/trees.ts` | `treeFailureText` |
| `GiftRefusal` | `house/gift-rules.ts` | `giftRefusalText`, including gift-specific tree wording |
| `LibraryRefusal` (shelf removal and offer admission) | `library/librarian.ts` | `libraryRefusalText` |
| `SettingsFailure` | `models/failure.ts` | `settingsFailureText` |
| `CatalogueFailure` | `models/failure.ts` | `catalogueFailureText` |
| `ProviderFailure` | `models/failure.ts` | `providerFailureText`, using `models/plainly.ts` for model and connection checks |
| `LoginFailure` (including provider failures) | `models/failure.ts`, re-exported by `models/logins.ts` | `loginFailureText` |
| `ModelFailure` (including provider failures) | `models/failure.ts`, re-exported by `models/check.ts` | `modelFailureText` |
| `LimitsRefusal` | `limits/rules.ts` | `limitsRefusalText` |
| `MoveFailure` (pending house recovery) | `realm/dealings.ts` | `moveFailureText` |
| `SpendFailure` (hamlet or task cap) | `spend/afford.ts` | `spendFailureText` |
| `ManifestFault` | `hand/manifests.ts` | `manifestFaultText` via its exhaustive `reasonText`; also accepts stored legacy strings |
| `SearchFailure`, `RunFailure`, their union `ToolFailure` | `tools/failure.ts` | `toolFailureText` |
| `LetterFailure` | `letters.ts` | `letterFailureText`, delegating refused addresses |

Small internal outcomes stay at their operation:

- `agents/merge-files.ts` owns `PrepareFailure`, its merge-specific
  `NotesFailure`, and `PendingFiles`. `prepareText` delegates record and
  tree refusals; `pendingText` renders journal recovery advice. Both use
  the local `notesFailureText` for unreadable notes.
- `agents/leaving.ts`'s private `EraseFailure` is translated inline into a
  notice receipt by the erasure deed.
- `letters.ts`'s `QuestionAdmission` carries a `question-cycle` refusal,
  rendered inline by `askAgent`.
- `realm/dealings.ts`'s `AgreementResult` combines record and decision
  refusals with `MoveFailure` and `agreement-address`, rendered with
  `moveFailureText` and `resolveFailureText`.
- `library/librarian.ts`'s private `CleanupFailure` is rendered by
  `cleanupFailureText` after publication.
- `chats/errands.ts`'s `ErrandOutcome` distinguishes `done` (with text) from
  `failed`, `aborted` and `timeout` (with a reason); its caller chooses the
  fallback or recovery words.

## The client boundary

[`src/protocol.ts`](../src/protocol.ts) defines the public wire shapes.
A failed request is answered with
`{ type: "reply", to: request.id, kind: "error", text }`.
`to` correlates the answer with the client's request. `fault` carries
failure text with an optional `to`, including failures while opening or
watching a conversation and connection-level failures. These envelopes
carry display text, not the internal error union, structured reasons or
a retry-safety flag. Put the safe reason and next action in that text;
do not invent public fields outside the protocol.

The window displays this text in the requesting panel or under the chat
field; an unaddressed fault is also shown as a chat failure.
`hamlet chat` prints error replies and faults as `error: <text>` on its
diagnostic stream. Server wording is therefore chief-facing copy, not a
debug log. Preserve request correlation so clients can place the failure
beside the action that caused it.

`src/server/envelope.ts` is the single policy point for chief-facing reply
and fault wording, reply kinds, request correlation and envelope scrubbing.
Its `ChiefFailure` union collects the chief-facing families above;
`replyFor` and `faultFor` use an exhaustive kind switch to select the
owning renderer, then call `Home.secrets.value` once. Successful receipts
are correlated and scrubbed there too. The wire never exposes the failure
union itself.

The envelope preserves the boundary's notice policies: typed conversation
refusals are notices while their protocol equivalents are errors; command
usage chooses notice or error by command; provider failures during model
checks are notices, and sign-in failures are errors. `say` turns an
unavailable typed answer into a notice receipt. Sign-in progress is a
separate protocol message: `server/sign-in.ts` uses `loginFailureText` and
scrubs progress with both current credentials and that session's secrets.

A thrown handler error reaches the connection as a defect: the server
logs its scrubbed cause with the request id and sends a correlated fault
through `faultFor`. Detached handler work uses `Client.fail` for that same
path. Expected refusals return typed outcomes rather than entering it.

Translate internal errors deliberately before sending them. Keep richer
diagnostics on the server, subject to the same rules about safe data.
Never send credentials, tokens, a provider's raw response, a stack trace,
private causes or paths outside the receiver's permitted reach. A useful
safe identity should survive redaction.

Select safe fields and wording first, then apply C32's credential
scrubbing before storage or display, including structured tool output,
streamed and spilled output and protocol messages. `Home.secrets` reads the
home's current credentials for each operation; its `words`, `value` and
`shell` capabilities scrub text, structured values
and shell output. Storage and display callers use that same capability.
Scrubbing does not make arbitrary provider text or private paths suitable
to send. A stream must also scrub credentials split between chunks. Test the boundary's output,
not just the scrubber in isolation.

Startup must establish its recovery invariants before accepting requests.
If `clearStagedGifts` refuses, the gift area could not be reset to an empty
server-owned stage. This is a server startup defect: serving stops with
the existing rendered refusal, rather than accepting uploads in that state.
If a committed house move cannot finish, recorded membership and its house
are not reconciled. That part's startup fails under the home's existing
part-start policy: the home stays open and reports its pending work.
During a running tool call, move recovery refusals remain `MoveFailure`
values. Hamlet cleanup returns `TreeFailure` to its per-hamlet reporter
and continues processing other hamlets.

## Errors read by an agent

A model reads a refused or failed tool call as part of its conversation.
Tell it which operation failed, the relevant safe reason, and what it can
do instead: correct an argument, read an existing path, ask a chief, or
choose a permitted tool. State uncertain effects and retry safety here
too. `tools/failure.ts`'s `toolDefect` logs a scrubbed private cause and
throws a safe message retaining that cause. Librarian defects use this
path because the harness includes thrown messages in model-visible tool
errors. Expected offer refusals use `libraryRefusalText`.

A bare exception name gives the agent no useful plan; automatic
retries can repeat unsafe effects. Recovery advice must respect the gate,
sandbox and the agent's permissions.

## Exhaustive boundary translation

Translate a typed error by switching on its `kind`. Give each kind an
explicit policy for public wording, safe context and recovery. Preserve an
error unchanged when it already belongs to the receiving contract.
Do not distinguish expected kinds with `instanceof`, string matching or
a catch-all that silently absorbs future variants.

For example, the two ways an agent name can fail C10 need different advice:

```ts
type NameFailure = {
    operation: "create-agent";
    retrySafe: false;
} & (
    | { kind: "name-format"; reason: "alphabet-or-length" }
    | { kind: "name-reserved"; reason: "reserved-address" }
);

function nameFailureText(error: NameFailure): string {
    switch (error.kind) {
        case "name-format":
            return "The agent could not be created: its name needs 1 to 24 lowercase letters or hyphens. Choose a name in that form.";
        case "name-reserved":
            return "The agent could not be created: that name is reserved for an address. Choose a different name.";
    }
    const unhandled: never = error;
    throw new Error(`Unclassified agent name failure: ${unhandled}`);
}
```

The `never` assignment makes an added kind fail compilation until its
translation is chosen. The throw is an unreachable defect check, not a
fallback reply. Put the translated text in the protocol's error envelope
and scrub it before sending. Catch untyped library exceptions only at
their boundary, classify known expected failures there, and retain defects
as defects.

## Review checklist

- Does the message identify the failed operation and a specific safe cause?
- Does the Result expose every expected kind, with useful typed context?
- Is there a next action for the chief, client or agent reading it?
- Are retry safety, partial effects and reassurance supported by evidence?
- Does every boundary handle all kinds with a `never` check?
- Can safe ids and the request id connect the failure to its diagnostics?
- Are secrets, private paths, raw provider data and stack traces excluded,
  with C32 scrubbing applied before storage or display?
- Does a boundary test cover the rendered outcome and any recovery advice
  whose correctness depends on effects or replay?
