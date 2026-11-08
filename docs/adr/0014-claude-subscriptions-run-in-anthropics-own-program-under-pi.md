---
status: accepted
---

# Claude subscriptions run in Anthropic's own program, under pi

A chief may run agents on their own Claude subscription. Anthropic allows
that only through its own program: an app may run the unmodified Claude Code
that the Agent SDK starts, and the chief signs in inside that program,
through Anthropic's own flow. An app may not offer its own Claude sign-in,
nor hold, store or pass on the chief's Claude credentials. pi's Claude
Pro/Max sign-in does both, which is why ADR 0006 keeps it off.

So pi has a provider `claude`, in `harnesses/pi/src/claude.ts`: each model
call of a `claude::` model is answered by Claude Code, run by
`@anthropic-ai/claude-agent-sdk` pinned to an exact release, unmodified,
with none of its sign-in methods removed.
Its models are pi-ai's Anthropic models, under the provider `claude`. Every
agent stays on pi; a Claude model is chosen like any other, so pi keeps the
transcript, compaction, recovery and every tool call (ADR 0002, ADR 0003).
User-facing text says the agent runs Claude, never that a feature of Hamlet
is Claude Code.

**Calls.** Claude Code's own tools are off (`tools: []`), no Claude Code
settings, MCP servers or slash commands load (`settingSources: []`,
`strictMcpConfig`, `verbatimPrompts`), and it compacts nothing
(`DISABLE_AUTO_COMPACT`). The conversation's tools reach Claude through an
in-process MCP server. When Claude calls them, the call ends as pi's answer
with those tool calls, and Claude Code's query waits in the MCP server.
pi runs the tools as with any provider; its next call brings their results,
which the waiting query takes and answers from. Claude Code makes its next
request only once it has those results, so each of its requests is one
call of pi, which the server allows first as `attempts` promises. Any
other call starts a new query.

**Sessions.** A query that answers its last call
in full leaves its Claude Code session kept for the next input of the same
pi session, with a digest of each of pi's messages it holds, the model and
a digest of the system prompt. The next query resumes it, given only the
new input, when pi's history before that input is just those messages and
the model and system prompt are the same. Else, as after a compaction, an
edit or a changed prompt, it resumes a session written from pi's history,
and the kept one is removed. The system prompt has no part that follows
spending (C11), so it stays the same from one input to the next. Kept
sessions live in the runtime's memory: a restarted runtime writes the
history again once for each conversation.

**Sign-in.** The provider's sign-in runs `claude auth login` with `BROWSER`
set to a one-line script that writes the sign-in link to a file, and sends
that link, and nothing else, to the window as a sign-in step. The chief
opens it, signs in on Anthropic's pages, and the browser returns to the
Claude program's own listener on the server's machine. The code and the
token stay in the Claude program. The server keeps a credential that only
marks that the sign-in was made: random words, no token. The login is
Hamlet's own, never the chief's `~/.claude`: the Claude program runs with
`CLAUDE_CONFIG_DIR` in the runtime's folder, and without the chief's
`ANTHROPIC_*` and `CLAUDE_*` variables, so each realm has one Claude login.

**Billing.** A call Claude Code reports as billed beyond the plan, to extra
usage, fails, and the query waiting for its results is not resumed. The
server prices Claude's models at their list prices, as it does ChatGPT's,
so budgets count what the agents do though the plan bills no tokens.

## Considered options

- pi's own Claude Pro/Max sign-in. Rejected: it takes the chief's Claude
  token into Hamlet and imitates Claude Code to use it, which Anthropic's
  terms forbid.
- An API key from the chief's Console organisation. Kept: it works today
  and bills the key; this provider is for the subscription.
- A second harness that runs Claude Code's whole agent loop, allowed by the
  turn. Rejected: the server could not allow each model call first, a turn
  could run past a spent budget, Hamlet's compaction and recovery would not
  apply, and every agent would need a harness of its own chosen for it.
- A loopback relay on `ANTHROPIC_BASE_URL`, holding Claude Code to one
  request per call. Rejected: the relay sees the
  authenticated requests and rewrites them.
- A session written from pi's history for every input. Rejected: each input
  would resend the whole conversation uncached and drop Claude's earlier
  thinking.
- Keeping a Claude Code session past history pi rewrote, cut at the last
  point both agree on.
  Not taken: Hamlet rewrites history only by compaction, after which little
  of the session would be kept.

## Consequences

- An input after a compaction, a changed model or a changed system prompt,
  or the first after a runtime restarts, resends the conversation to Claude
  Code uncached.
- The sign-in completes only in a browser on the server's machine, since the
  Claude program listens there for Anthropic's answer.
- Signing out in Hamlet forgets the marker, not the Claude program's login.
- The Claude program is not in Hamlet's release archives: its packages are
  optional, and pi offers Claude only where they are installed.
- A hosted Hamlet needs Anthropic's agreement first, as ADR 0006 says of
  ChatGPT's sign-in.
