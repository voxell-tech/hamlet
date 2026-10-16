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

`src/server/envelope.ts` translates returned chief failures into replies
or faults and scrubs each envelope once. Its exhaustive switch delegates
wording to the owning family. A thrown handler error reaches the
connection as a defect: the server logs its cause with the request id and
sends a correlated fault. Detached handler work uses the same defect path.
Narrow compatibility catches at owner calls retain error replies for
unclassified refusals. Typed conversation commands and typed answers retain
their notice receipts for those refusals. Scrubbing stays in the envelope.

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
and shell output. The storage helpers `safeWords` and `safeValue` in
`src/scrub.ts` use that same capability. Scrubbing does not make arbitrary
provider text or private paths suitable to send. A stream must
also scrub credentials split between chunks. Test the boundary's output,
not just the scrubber in isolation.

## Errors read by an agent

A model reads a refused or failed tool call as part of its conversation.
Tell it which operation failed, the relevant safe reason, and what it can
do instead: correct an argument, read an existing path, ask a chief, or
choose a permitted tool. State uncertain effects and retry safety here
too. A bare exception name gives the agent no useful plan; automatic
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
