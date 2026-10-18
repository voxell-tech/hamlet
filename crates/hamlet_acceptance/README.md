# hamlet_acceptance

The acceptance suite for Hamlet's server. Each scenario starts a server
process on a scratch home and checks what it does from outside: through
protocol v0 over the authenticated WebSocket, with `hamlet_protocol`'s
generated types, and through the `hamlet` command. It knows nothing about
how the server is built, so the same scenarios judge the TypeScript server
in `server/` and the Rust server that replaces it.

Passing these scenarios is not yet sufficient to delete `server/`.
The coverage map below includes untested observable behaviour, including
gate edge cases, accounting rollover and CLI lifecycle.
Home migration and rollback also need their own checks before switching,
as required by the Rust-server proposal. `covered` means the named TS
test's observable assertions are represented, not that its entire domain
is proved. Neither file counts nor a green run waive the gaps.

Every message a server sends is parsed into `ServerMessage`, written back
and compared with what arrived, so a field outside protocol v0 fails the
scenario that received it.

## Running it

The scenarios are compiled only with the `acceptance` feature, so plain
`cargo test` neither builds nor runs them. They need the built `hamlet`
command and, for today's default server, Node and
`npm ci --prefix server`. `make check` runs them. By hand, at the root:

```sh
CARGO_INCREMENTAL=0 cargo build -p hamlet_cli
CARGO_INCREMENTAL=0 cargo test -p hamlet_acceptance --features acceptance
```

They never contact a model server or the internet. Models are faux, or the
loopback endpoint in `src/openai.rs`.

| Variable | Selects |
|---|---|
| `HAMLET_ACCEPTANCE_SERVER` | The command that starts a server, split on whitespace. Unset or empty: the `hamlet` command's `server`. |
| `HAMLET_CLI` | The `hamlet` command, for the default server and the CLI scenarios. Unset: `target/debug/hamlet`. |

A relative executable path is resolved against the workspace root. To
judge the Rust server with the same scenarios:

```sh
CARGO_INCREMENTAL=0 cargo build -p hamlet-server -p hamlet_cli
CARGO_INCREMENTAL=0 HAMLET_ACCEPTANCE_SERVER=target/debug/hamlet-server cargo test -p hamlet_acceptance --features acceptance --no-fail-fast
```

A server that cannot be started fails its scenario, with the end of what
it printed. Nothing is skipped.

## Starting a server

This is the contract a server command keeps to be judged by the suite.

The suite runs the command with these arguments added:

- `--port 0`, to listen on a free loopback port.
- `--faux <file>`, the absolute path of a faux script (below). Without it,
  the server uses its real model providers; the suite then points them at
  a loopback endpoint with `HAMLET_MODEL` and `HAMLET_BASE_URL`.

Its environment is the suite's own with ambient `HAMLET_SERVER`,
`HAMLET_TOKEN`, `HAMLET_MODEL`, `HAMLET_BASE_URL`, `HAMLET_API_KEY`,
`HAMLET_NET`, `HAMLET_REFLECT_EVERY`, `HAMLET_WORKSPACE_URL` and
`HAMLET_WORKSPACE_TOKEN` removed. These overrides must not connect a test
to the chief's server or remote workspace. The suite then sets:

- `HAMLET_HOME`: the home, a fresh scratch folder, or the folder a
  previous server of the same scenario used.
- `HAMLET_USER=chief`: the chief's name.
- What the scenario adds: `HAMLET_REFLECT_EVERY=0` unless it says
  otherwise, and `HAMLET_MODEL`, `HAMLET_BASE_URL`, `HAMLET_NET` or
  `HAMLET_API_KEY` where it tests them.

Stdin is closed. Stdout and stderr go to a log the suite shows on failure.

The server is ready when `<home>/server.json` holds
`{"url": ..., "pid": ..., "started": ...}` with `pid` the process the suite
started:

- `url` is the WebSocket address to connect to.
- `started` is the start time in UTC, as an ISO time to the millisecond.
- It holds no token.

The token is `connection.token` in `<home>/credentials.json`, a file only
its owner can read or write (mode 0600). The token stays the same across
servers on one home. The suite waits up to 30 seconds for `server.json`,
and fails at once if the process ends first.

One server serves a home at a time. A second one on a served home ends
with an error that says `A server already runs for <home>`.

At SIGTERM, or a first SIGINT, the server finishes in order:

- It closes every connection with code 1001.
- It removes `server.json`.
- It exits with 0.
- The next server on the home carries on what was cut off.

A second SIGINT ends it at once with 130. SIGKILL may leave `server.json`
behind; the next server replaces it.

### Files exposed to the scenarios

The suite also checks the public local-workspace files from
[`docs/home.md`](../../docs/home.md), which the Rust-server proposal keeps:

- `houses/<hamlet>/<number>/NOTES.md`, `skills/` and `inbox/`. The first
  agent is `home:0`. `state.houses` names the absolute houses directory.
- Arbitrary files made by tools or gifts in those houses. Merge scenarios
  check the retained copies in `merged/<absorbed-name>/`, including
  `NOTES.before.md` and conflicting files.
- `library/<skill>/SKILL.md` and the library path in the composed prompt.

Scenarios seed notes, skills and files there and check their bytes,
executable modes, moves, copies and removal. Work folders, editor scripts,
faux files and release markers live outside the home in scratch folders.
No scenario opens a database, a harness store or `models.json`, or uses
server log text for synchronisation. The only asserted startup diagnostic
is the duplicate-home refusal above; a linked houses root must also fail
startup without changing the link's target. The sandbox scenarios require
Linux, working bubblewrap, git, curl and POSIX shell tools, not a Node
server. They plant outside canaries and symlinks, use a loopback listener,
and inspect tool-created files and protocol outcomes.

## Faux scripts

A faux script is a JSON object of answers given in a model's place, so a
scenario knows what each model call returns. The Node worker that runs
pi-durable for the Rust server reads the same format. Every field is
optional, and no others are allowed:

| Field | What it holds |
|---|---|
| `models` | Nonempty array of distinct, nonempty model ID strings. Defaults to `["faux-1"]`. |
| `responses` | Steps for any request without a list of its own, in order. |
| `agents` | Steps for each agent, by `"<hamlet>:<number>"`, for example `"home:0"` for the first mayor. |
| `errands` | Steps for one-off conversations: ballots, reviews of a merge, merged notes and the library's calls. |
| `price` | `{"input": dollars, "output": dollars}` per million tokens. Without it, the faux model is priced as the catalogue assigns. |
| `tokensPerSecond` | How fast answers stream. Without it, at once. |

A request takes the next step of its list. An agent's list is the one for
the house its conversation works in; a request with no list of its own
takes from `responses`. A list that has run out answers with a model error,
`No more faux responses queued`, and never falls back to `responses`.

Lists are shared across every conversation of that agent, including chief,
letters, questions and reflection. All one-off conversations share
`errands`, which takes precedence over an agent list. Model checks with no
agent use `responses`. Missing `responses` means an empty list; an explicit
empty agent or errand list also fails immediately. Each call consumes its
step when the provider is invoked, before waiting or streaming, even if it
later fails or is cancelled. Concurrent calls take successive steps in
invocation order. Each new server process starts fresh lists from its
script, including after a crash; list positions are not durable.

Queue selection is by conversation identity, not by arbitrary prompt
words. The TS adapter obtains that identity from the `machine` section's
house path and identifies errands by their `instructions` section. A
second implementation may pass identity directly; it need not copy this
prompt inspection. Moving an agent changes its queue key to its new house.

A step is a string, which is the answer's words, or an object:

| Field | What it does |
|---|---|
| `text` | The answer's words. |
| `thinking` | Its reasoning. |
| `calls` | Tool calls, `[{"name": ..., "arguments": {...}}]`. An answer with calls ends with "tool use", one without with "stop". |
| `error` | A model failure with these words, in place of the answer's ending. |
| `stallMs` | Milliseconds the answer takes. Stopping the request does not cut this short. |
| `holdUntil` | A file the answer waits for, relative to the script's folder. Stopping the request ends the wait. |

`text`, `thinking` and `error` are strings, including empty strings.
`calls` is an array; each call has exactly `name` (string) and `arguments`
(a JSON object). Content is reasoning first, text second, then calls in
array order; omitted fields produce no block. `error` retains that content
but ends with an error rather than stop or tool use. `stallMs` is a
nonnegative integer and runs before `holdUntil`; the latter is a nonempty
path, absolute or resolved relative to the script file. Its existence,
not its contents, releases the response. Cancellation consumes the step
and emits no remaining content, but cannot interrupt `stallMs`.

`agents` keys match `^[^:]+:[0-9]+$`. Prices are nonnegative finite numbers;
`tokensPerSecond` is a positive finite number. Unknown fields in the script,
steps, prices and calls are rejected. These additions preserve old scripts
containing only `responses`, `tokensPerSecond` and the original step fields.

Each `models` ID is offered as `faux::<id>`, in array order, with a
128000-token context and a 16384-token output limit. IDs are case-sensitive,
not trimmed, and are not endpoint-qualified in the script. Omitting
`models` preserves the original single model. The default selection with
`--faux` remains `faux::faux-1`, so scripts using that selection must include
`faux-1`. `models` replaces the listing; it does not append to it.
All models use the same `price`, streaming rate and queues. Switching
models does not reset or choose another queue: the next call consumes the
next step for its conversation identity, even if the previous step used
another model. There are no per-model steps or prices.

The pi Node harness uses pi-ai 1.2.0's faux
provider for usage estimates and streaming. Chunk boundaries, generated
tool IDs and timestamps are not fixed by a script. Rates default to zero;
catalogue price overrides apply to successful responses, with cache rates
zero. Usage is estimated from the actual prompt, so the scenarios assert
accounting relationships rather than fixed faux token counts. The loopback
OpenAI fixture supplies fixed usage for exact billing assertions.

### Loopback endpoint

`src/openai.rs` lists `org/large` with a 32000-token context by default.
`list` replaces its `/models` response. `enqueue` queues completion
responses in request-arrival order, consumed before any hold. An empty
queue answers `Hello.`. Responses can set HTTP status, text, finish reason
(including `length`), OpenAI tool calls and final usage, or wait for an
absolute marker path. Defaults are HTTP 200, `stop`, 100 input tokens,
20 output tokens and a $0.0015 bill. `usage: None` omits usage; explicit
zero cost is distinct from missing cost. Listings never consume completion
responses. Dropping the endpoint closes held requests too.

The fixture records requests at the external provider boundary to check
model choice, retry, compaction and the actual prompt. This is not access
to the server's internal transcript. Ordinary tool output is not exposed
in protocol v0, but can be checked in the next model request when needed.

## Scenarios

| File | What it covers |
|---|---|
| `tests/mayor.rs` | Chat and turns, tools, approvals and standing permissions, faults, retry, stop, rewind, new and resume, paging, budget stops, questions, crash and orderly stop, activity |
| `tests/documents.rs` | The charter, characters, notes, history, the prompt, limits, settings, the catalogue, the folder and mode, refusals of unknown targets |
| `tests/connection.rs` | The hello and token, malformed and oversized messages, broadcasts, closing, `server.json` and `credentials.json`, one server per home |
| `tests/society.rs` | Letters, replies and questions, roads, protection, removing, restoring, erasing and merging agents, votes, objections, the library, what the mayor may create |
| `tests/realm.rs` | Founding hamlets by command and by vote, roads and moves between hamlets, removal asks, closing, restoring and erasing hamlets, their papers |
| `tests/gifts.rs` | Files given in parts, folders, refused paths and manifests, upload bounds, starter and copied skills, gifts to departed agents |
| `tests/models.rs` | Model checks, grants, key logins, changing listings and catalogue persistence, model selection and pins, retry, usage, compaction and the actual prompt |
| `tests/cli.rs` | `hamlet chat` on stdin and stdout, approvals across a crash, switching agents and hamlets, `hamlet edit`, `hamlet models`, `hamlet login` |
| `tests/sandbox.rs` | Linux read/write/network confinement, symlinks and missing roots, house versus project files, resource limits, timeout cleanup and hand-added tools |

## Coverage of the server's tests

Every file in `server/test/` with what it checks that a client of protocol
v0 or the `hamlet` command could see. A file name in Where, such as
`mayor.rs`, is a file in `tests/`, followed by its scenarios.

Status:
- `covered`: a scenario asserts the observable behaviour.
- `partly`: some of it is asserted. Where says what is missing.
- `not yet`: the behaviour is observable, but no scenario checks it. Where says why.
- `internal-only`: the file checks internals, pure functions, module structure, sandbox internals or races inside the process.

| File | Observable behaviour it checks | Status | Where |
|---|---|---|---|
| acceptance.test.ts | Mayor via `hamlet chat`: gate, house note, reflect, budget, stop, crash | partly | `cli.rs` asks_for_approvals_and_keeps_them_across_a_crash (held edit, `/allow`, `/budget`, auto bash, `/stop`, wait kept across SIGKILL, chat exits 1, new chat shows wait); `mayor.rs` reflects_when_the_chief_asks. Missing: a house NOTES.md edit not held in manual mode. |
| activity-connection.test.ts | Agent activity tells reading from writing; saying keeps last 600 bytes | covered | `mayor.rs` tells_what_the_mayor_is_doing (notes, search/find as read-file, read-skill, steps, failures and the UTF-8 600-byte tail). |
| address.test.ts | Resolves office and missing/departed/malformed addresses | partly | Refusal words appear in `documents.rs` and `society.rs` (`no agent named`, farewells). Missing: malformed and non-text addresses and office resolution after handover. Direct resolver calls are internal, but their outcomes are observable. |
| agent-chats.test.ts | Chief watches agent conversations: read-only, paged, scrubbed, counted, authored | partly | `society.rs` carries_a_letter_and_its_reply_then_asks_a_question (letters by default, questions by kind, authors, rewind refused, count 1); `realm.rs` founds_a_hamlet_by_vote_has_the_asker_pay_and_overturns_it (asked side counts). Missing: paging a peer chat, no turns after `close`, secret scrubbed, author named after moves. A foreign chief's watch needs a second chief. |
| agents.test.ts | `create_agent`/`set_model`: names, character, model, roads, refusals, reserved | partly | `society.rs` refuses_agents_the_mayor_may_not_create, asks_before_a_reserved_creation_and_caps_the_hamlet; `models.rs` applies_allowed_model_choices_and_refuses_a_pinned_choice (character, allowed/forbidden choice, pin and revocation). Missing: all refusal words and denial of a reserved creation. |
| answers.test.ts | Empty answer, cut-off summarising, repeated model fault until dismissed | covered | `mayor.rs` empty/thinking-only answers and repeated fault with `asked:2`; `models.rs` cutoff success, second cutoff, empty summary, no history, automatic 80% compaction preserving the exact line, cutoff reflection without retry, and charges_a_summary_to_the_cut_off_task_not_the_queued_line (cap stops the original task while the queued line answers). |
| brief.test.ts | Hamlet section of prompt: agents, instructions, roads, decisions, merges | partly | `society.rs` briefs_each_agent_on_its_hamlet, merges_houses_by_command_keeping_both_sides, keeps_a_refused_road_beside_six_recent_decisions (including toldDays=0); `realm.rs` cross-hamlet roads; `models.rs` gives_only_a_resident_the_civic_reflection_question. Missing: a positive multi-day ageing boundary. |
| budget.test.ts | Budget words, daily and task caps, model-time caps, errand payer | covered | `mayor.rs` budget/task stops and a cap lowered mid-run; `models.rs` fixed per-turn budget words, charges_the_librarian_to_its_callers_hamlet_and_task (including budget before task when both are exhausted), lowering_the_budget_prevents_cutoff_compaction; `society.rs` stops_at_a_model_time_cap_charged_by_a_road. |
| catalogue.test.ts | Chief price, allowance, note survive a listing refresh and restart | covered | `documents.rs` prices_allows_and_notes_a_model_in_the_catalogue; `models.rs` keeps_catalogue_edits_when_listings_change_and_the_server_restarts, lists_prices_and_bills_an_endpoints_models. |
| change.test.ts | Diff summary of an edit or write: marks, counts, caps | internal-only | Pure `changeBetween`/`changeOf`. Its result is asserted as `wait.change` in `mayor.rs` holds_a_command_and_an_edit_for_approval_in_manual_mode. |
| chat.test.ts | `hamlet chat` rendering: waits, streams, switching, help, tty, EOF, Ctrl-C | partly | `cli.rs` chats_with_answers_on_stdout_and_the_rest_on_stderr, asks_for_approvals_and_keeps_them_across_a_crash (standing wait shown at connect), starts_at_another_hamlet_and_switches_agents_and_hamlets. Missing: answer strips, dimmed reasoning and exit lines, help listing, login advice with no model, EOF waiting for its own lines, Ctrl-C leaving the mayor working, only the last answer on reconnect, `/new` before EOF, pinned past view. The tty prompt needs a pty. |
| chats.test.ts | Chat persists across reopen; one chat per chief; same id once | partly | `mayor.rs` admits_a_line_sent_twice_with_one_id_once, keeps_a_wait_and_the_chat_across_a_crash. One chat per chief needs a second chief. Entry dates are internal. |
| check.test.ts | Model and connection hello, plain error words, 90 s timeout, scrubbed link | partly | `models.rs` tries_the_model_and_the_connection_and_keeps_the_link, says_plainly_when_the_model_answers_no_hello, gives_plain_words_for_http_refusals_and_charges_nothing (401/402/403), scrubs_successful_and_failed_hellos_before_publishing_the_link. Missing: the real 90 s timeout. |
| chief-access.test.ts | Refusals for gifts, chat deeds and answers in removed/erased/foreign hamlets | partly | `realm.rs` edits_and_reads_another_hamlets_papers_until_it_is_removed (edit limits, edit notes, read prompt after removal). Missing: gifts and chat deeds after removal or erasure. The foreign-hamlet cases need a second chief. |
| choice.test.ts | Which model runs: pin, hamlet model, home default, decisions | covered | `models.rs` the_chiefs_pin_wins_and_survives_a_held_request_and_restart (actual requested model, hierarchy, decisions in the brief, change between tool rounds), applies_allowed_model_choices_and_refuses_a_pinned_choice, gives_only_a_resident_the_civic_reflection_question (constructor as an agent name and pin key, changed override after restart), summarises_at_eighty_percent_before_requesting_the_next_answer. Direct inspection of the compaction reserve field is internal. |
| cli-main.test.ts | `--help` and a usage failure, with isolated streams | covered | `cli.rs` prints_its_usage_on_stdout (`--help` with empty stderr, `server --port high` with empty stdout). |
| cli.test.ts | CLI flags, one server per home, chat on stdin, server.json, SIGINT | covered | `cli.rs` prints_its_usage_on_stdout (every flag mistake and `-h` form), chats_with_answers_on_stdout_and_the_rest_on_stderr, ends_with_a_failure_when_its_agent_does_not_exist, prints_a_notice_for_people_on_stderr; `connection.rs` keeps_its_token_and_refuses_a_second_server_on_one_home, publishes_its_address_without_the_token; `mayor.rs` stops_in_order_at_ctrl_c_too, ends_at_once_at_a_second_ctrl_c. |
| commands.test.ts | Typed command parsing and the reply words of each deed | partly | `mayor.rs` answers_typed_commands_that_change_nothing, keeps_words_from_a_waiting_mayor_and_answers_by_command, answers_stop_commands_in_words, starts_a_new_session_and_resumes_the_first, carries_on_after_a_failure_with_retry, refuses_to_retry_while_a_turn_runs, stops_a_turn_when_the_budget_runs_out, gives_and_takes_back_a_standing_network_permission, names_a_permission_the_limits_give, answers_a_question_the_mayor_asks. Missing: typed `/dismiss` words, scrubbed typed words, the `/resume` list, `/budget` after spend. Queue order with waited seconds and founder-held words need a second chief and a clock. |
| confined-run.test.ts | `runConfined` scrubs streamed output, reports timeout | internal-only | Uses a `SandboxEnv` with `wallSecs:1` directly. |
| confinement.test.ts | House readers and tree workers keep to paths; notes kinds | internal-only | Direct house and tree workers. Folder refusals are seen in `documents.rs` points_the_hamlet_at_a_folder_and_switches_its_mode. |
| connection.test.ts | Hello, token, faults, close codes, ordering, broadcasts | partly | `connection.rs` all tests (wrong token, before hello, `v:1`, hello timeout, 1 MiB close, malformed faults, second hello, broadcasts, 1001 on stop); `society.rs` question recovery also checks strictly increasing revisions when joining a recovering server. Missing: a message sent just before close still handled, and a non-reading client not delaying shutdown. Handler-error cases are internal. |
| conversation-deeds.test.ts | Session numbering; retry/resume refusals; no chat made on refusal | partly | `mayor.rs` starts_a_new_session_and_resumes_the_first, starts_and_resumes_sessions_by_message, carries_on_after_a_failure_with_retry. Missing: a refused resume on a fresh agent creates no chat. |
| credentials.test.ts | credentials.json shape, mode 0600, kept token, atomic rewrite | partly | `connection.rs` publishes_its_address_without_the_token (mode 0600), keeps_its_token_and_refuses_a_second_server_on_one_home (same token). Missing: file shape, dropped `git` field, ignored temp file, `HAMLET_API_KEY` never written. |
| danger.test.ts | Gate rule: which calls run, which ask and with what danger | internal-only | Pure rule. Its waits are asserted in `mayor.rs` holds_a_command_and_an_edit_for_approval_in_manual_mode and gives_and_takes_back_a_standing_network_permission, and in `society.rs` asks_before_a_reserved_creation_and_caps_the_hamlet. |
| dealings.test.ts | Cross-hamlet roads and moves both mayors agree to; refusals | partly | `realm.rs` agrees_a_road_and_a_move_between_hamlets (road, move with house, notes, chat, kept roads). Missing: refusal texts, a half-done move finished on restart, an open vote blocking, an unsplit merge, room for two, a departed name, the other mayor's prompt words. |
| deeds.test.ts | Chief deeds during a held wait; typed and structured deeds match | partly | `mayor.rs` answers_stop_commands_in_words (`/stop founder` closes the wait), starts_and_resumes_sessions_by_message. Missing: edits during a held wait, `dismiss` refused on an approval, retry after a stop, typed and structured runs giving the same entries. |
| documents-cli.test.ts | `hamlet edit <doc>` round-trips through EDITOR | partly | `cli.rs` edits_limits_in_editor_and_reports_a_refusal, edits_whole_documents_in_editor_and_reads_them_back, edits_settings_as_whole_json_without_credentials. Missing: a saved settings change, refused `model:null`, the temp file's 0600 mode and removal. The terminal editor needs a pty. |
| edits.test.ts | Limits edits by key or whole; change messages; refusals | partly | `documents.rs` edits_limits_one_key_or_whole_and_refuses_bad_values (including the nested list refusal). Missing: `__proto__` refused as an unknown field. |
| envelope.test.ts | Reply and fault shapes, scrubbing, stable refusal words | partly | `documents.rs` refuses_requests_for_hamlets_and_agents_that_do_not_exist; `mayor.rs` gives_and_takes_back_a_standing_network_permission, answers_a_question_the_mayor_asks, carries_on_after_a_failure_with_retry; `society.rs` removes_restores_and_erases_an_agent_by_command, lays_and_takes_up_roads_by_command, merges_houses_by_command_keeping_both_sides, hands_the_office_over_by_command, shelves_an_offered_skill_for_every_agent_and_unshelves_it. Missing: `/to` usage, a scrubbed read refusal, `/remove nobody`, `/found all`, structured `retry` after an answer, byte equality with recorded frames. |
| environment.test.ts | Shell working folder, sandbox reach, network, relative path footing | covered | `mayor.rs` manual folder commands and edits; `sandbox.rs` confines_reads_writes_and_links_with_and_without_a_folder, reaches_a_listener_only_for_the_approved_network_call, keeps_notes_and_skills_in_the_house_and_git_files_in_the_folder. |
| errands.test.ts | Errand payer, kept answer, failure, timeout, abort | partly | `society.rs` accepted library and merge errands; `models.rs` charges_the_librarian_to_its_callers_hamlet_and_task reconciles exact spend, stops the task and admits the next. Missing: kept answer after reopen, failure, timeout and abort. |
| faux.test.ts | Legacy parsing, queue-key validation, concurrent dispatch, exhausted and absent queues, prompt-safe routing | internal-only | Adapter contract tests; no server behaviour beyond the faux format above. |
| file-helper.test.ts | Sandbox file-helper framing, FIFO limits, reaping | internal-only | Fake transport, timers and PIDs. |
| foundation-chief.test.ts | A home keeps chief authority when `HAMLET_USER`/`USER` changes | not yet | Needs restarts under other `HAMLET_USER`/`USER` values. The harness always sets `chief`, and no scenario overrides it. The migration case is internal. |
| foundation-deeds.test.ts | Stop agents in other hamlets; answers routed to the right wait | partly | `realm.rs` stops_another_hamlets_agent_by_name_and_office exercises `/stop scout`, `/stop scout@mill`, `/stop mayor@mill` and stopped waits. Foreign-chief and raised-question cases still need a second chief or internals. |
| founding.test.ts | Found, name, place, remove, restore, erase hamlets; removal asks | partly | `realm.rs` founds_names_and_places_hamlets_by_command, gathers_removal_asks_and_lets_only_the_chief_close_restore_and_erase, agrees_a_road_and_a_move_between_hamlets (cross-hamlet road words), sends_a_whole_state_when_a_hamlet_closes_or_goes. Missing: removal stopping work and lapsing waits, restore refused when the name is taken, erase then restart, refused cleanup through a symlink, a repeat ask counted once. The other-chief case needs a second chief. |
| frames.test.ts | Recorded frame files parse as v0 and fit each scenario | internal-only | Checks the fixture files in `crates/hamlet_protocol/frames/`, recorded in-process. Their live behaviours are re-asserted in `mayor.rs`, `society.rs`, `realm.rs` and `gifts.rs`. |
| gate.test.ts | Approval gate: what is held, previews, trust, refusals, grants | partly | `mayor.rs` manual approvals, denial, grants, keeps_edit_trust_and_network_refusal_across_restart_but_not_resume (edit-only trust, refused kind retained); `models.rs` sends_grants_from_limits_and_from_answers. Missing: credentials in held text, linked paths, lowered asking limit, three refusals, binary/empty previews and the office in `permissions.network`. |
| gift-uploads.test.ts | Upload pipeline races: disconnects, inbox swapped for a link | internal-only | Mocks `fs` and calls the uploads directly. |
| gifts.test.ts | Typed `/give` of a starter: files, record, no turn, refusals | partly | `gifts.rs` installs_a_starter_skill_and_shows_it_to_every_connection (notice, refusals, no turn). Missing: files equal the shipped assets, refusal after a restart, a link at the destination, a linked skills folder. |
| give.test.ts | `give`/`give-part` file and skill gifts: limits, paths, scrubbing | partly | `gifts.rs` all tests (parts, folders, `-2` names and the agent's line, bad paths and manifests, collisions, 40 MiB, offsets, per-connection and per-agent bounds, exactly 32 MiB, starters, another agent's skill with mode, typed giver, departed recipient). Missing: simultaneous gifts, a secret in a file name, skill listing not following links, FIFO skill paper, links/size/entry limits on skills, idle expiry, rollback, linked or deleted inbox, a giver in another hamlet, 4097 entries. |
| grants.test.ts | Grants survive reopen; second revoke records nothing | partly | `mayor.rs` gives_and_takes_back_a_standing_network_permission checks the grant flow. Missing: survival across restart and repeated revoke recording nothing. These outcomes are observable even though the TS test calls the grant store directly. |
| hand.test.ts | Hand-added tools: reserved names, reach, limits, schema, reload | partly | `sandbox.rs` loads_hand_tools_checks_arguments_and_confines_their_reach (reserved diagnostic, schema, stdin JSON, cwd, read-only tool even in fs_rw), reloads_hand_additions_edits_and_removals. Missing: linked tools, manifest-specific CPU/wall/net/env/read rules, concurrent private scratch and chunked secret scrubbing. Loader-slot injection is internal. |
| home.test.ts | First layout, default limits/settings, env overrides, single-server lock | partly | `documents.rs` starts_with_the_default_limits (layout and limits); `connection.rs` keeps_its_token_and_refuses_a_second_server_on_one_home; `mayor.rs` keeps_a_wait_and_the_chat_across_a_crash (a dead holder's lock taken over). Missing: default settings without `--faux`, in-memory `HAMLET_MODEL`/`HAMLET_NET`/`HAMLET_BASE_URL`, a bad `HAMLET_REFLECT_EVERY`, the lock through path aliases, the `tools` folder, the Rust app socket. |
| house-files.test.ts | House tree staging, delivery, listing, removal helpers | internal-only | Pure helpers, `fs` spies, injected races. |
| leaving-start.test.ts | Home opens when one part's start throws | internal-only | Injected fake parts. |
| leaving.test.ts | `/remove`, `/restore`, `/erase`, `/undo`, `/record`; protected tasks | partly | `society.rs` removes_restores_and_erases_an_agent_by_command, protects_an_instructed_agent_then_removes_and_undoes. Missing: `/remove nobody`, blank reason, concurrent removal, departure stopping a turn and its waits, a full hamlet on restore, the founder back under its own name, restore under a taken name, grants cleared, "has only just ended". |
| ledger.test.ts | Spend ledger: meters, dating, midnight, concurrency, road charges | partly | Totals in `mayor.rs` budget and rewind scenarios, `models.rs` billed/zero/unreported usage and librarian task charges, `society.rs` roads and model-time cap. Missing: concurrent accounting and daily/monthly rollover. Rollover needs clock control; the ledger's storage representation is internal. |
| letters.test.ts | `tell_agent`/`ask_agent`: replies, caps, refusals, cycles, sender marks | partly | `society.rs` carries_a_letter_and_its_reply_then_asks_a_question (reply author, letter and question items, road count). Missing: the task message cap, reused question chats, a cyclic question finishing, sender marking of chief-like text. Most refusal texts are tool output and never reach the wire. |
| librarian.test.ts | `submit_skill`/`ask_librarian`: shelve, refuse, unfit, budget, races | partly | `society.rs` shelves_an_offered_skill_for_every_agent_and_unshelves_it; `models.rs` charges_the_librarian_to_its_callers_hamlet_and_task (`ask_librarian`, task exhausted). Missing: refused verdicts, unfit offers, starter unshelved after restart, offering after stop, changes while judged, read-only staging, sweeping and verdict forms. |
| library.test.ts | Starters installed once; edits kept; Library prompt section; read-only | partly | `documents.rs` Library heading; `gifts.rs` starter installation; `sandbox.rs` library readable but not writable. Missing: exact starter files, chief edits/deletions kept across restart and starter lines in the prompt. |
| limits.test.ts | Limits edits keep defaults, refuse atomically, votes and hamlets | partly | `documents.rs` edits_limits_one_key_or_whole_and_refuses_bad_values, starts_with_the_default_limits (votes and hamlets defaults). Missing: `passOf 0`, `ballotMinutes -1`, `hamlets.most 1.5`, `value:"no"`, a whole votes/hamlets edit. The migration is internal. |
| logins-connection.test.ts | Key login streams without sending the key; reply bound; cancel; OAuth | partly | `models.rs` signs_in_with_a_key_without_sending_it_to_any_client (prompt, done, endpoint key, providers, no key sent, foreign reply refused, cancel notice and failed step). OAuth URLs, codes and scrubbed failure need an OAuth fake. |
| logins.test.ts | OpenAI key beside a ChatGPT OAuth credential; no secrets in storage | partly | `cli.rs` signs_in_with_a_key_lists_models_and_tries_a_hello and `models.rs` signs_in_with_a_key_without_sending_it_to_any_client (key stored). Missing: `home.sqlite` free of the key, ambient `OPENAI_API_KEY` ignored, the anthropic subscription refusal. Keeping it beside OAuth needs an OAuth fake. |
| manifests.test.ts | Hand manifest parsing and fault words | internal-only | Pure `parseManifest`/`manifestFaultText`. |
| membership-cap.test.ts | `mostAgents` caps `create_agent`, `/restore`, `/undo`, splits | partly | `society.rs` asks_before_a_reserved_creation_and_caps_the_hamlet (a capped `create_agent` makes nobody). Missing: the cap's failure text, and refusals on restore, undo and split. |
| merge-plan.test.ts | Merge planner: joined notes, clashes, renames | internal-only | Pure `planMerge`. Outcomes are seen in `society.rs` merges_houses_by_command_keeping_both_sides. |
| merging.test.ts | `/merge`, `merge_agents`: houses, clashes, notes errand, splits, refusals | partly | `society.rs` merges_houses_by_command_keeping_both_sides, merges_an_agent_redirects_its_letters_and_splits_it_back_out. Missing: skill clash renames, characters kept, notes rewrite accepted or rejected in `/record`, notes joined when the budget is spent, split latest first, link and size refusals, blocked destinations, non-UTF-8 notes, concurrent merges, a vote during the errand. Pending-merge recovery is internal. |
| models-cli.test.ts | `hamlet login`, `models`, `models try`, `models refresh` | partly | `cli.rs` key login, listing and hello success/failure; `models.rs` keeps_catalogue_edits_when_listings_change_and_the_server_restarts runs CLI refresh after a listing change. OAuth needs a fake; tty key input needs a pty. |
| models-connection.test.ts | Grants, link states, catalogue and settings edits, refusals, listings | partly | `models.rs` sends_grants_from_limits_and_from_answers, tries_the_model_and_the_connection_and_keeps_the_link, lists_prices_and_bills_an_endpoints_models, signs_in_with_a_key_without_sending_it_to_any_client; `documents.rs` edits_settings_and_refuses_bad_ones, prices_allows_and_notes_a_model_in_the_catalogue, refuses_requests_for_hamlets_and_agents_that_do_not_exist. Missing: the providers list, a non-unique endpoint name, `check` words for a bad model, listing forms, bad base URL overrides. Port-in-use and `isLocal` are internal. |
| models.test.ts | Real endpoint: retry, overrides not kept, billed usage, estimates | partly | `models.rs` billed usage, explicit zero, unreported usage estimates including history/tools, 503 retry with identical body, failed attempt at zero spend, held request across restart and Bearer header. Missing: overrides not persisted, stored endpoint restored without overrides and unusable overrides. |
| objections.test.ts | `object`/`ask_chief`, mayor review, waits, limits, restarts | partly | `society.rs` holds_an_instruction_for_an_objection_then_goes_or_withdraws; `mayor.rs` answers_a_question_the_mayor_asks. Missing: three questions a turn and a dismissal, the objection after a restart, mayor review (now scriptable with errands), late and outlived objections, two objections, another hamlet's `by`, a letter receiver held, retry that asks. Remote-mode reviews need a non-loopback endpoint. |
| ollama.live.test.ts | One real tool call against a live local model | not yet | Needs a live Ollama model; deliberately outside this offline suite. |
| one-command.test.ts | Bare `hamlet`, `stop`, `status`: detached server, window, stale records | not yet | Needs bare `hamlet`, `hamlet stop` and `hamlet status` with a fake `HAMLET_APP` window. No scenario runs them. |
| oversight.test.ts | Waits store queries, ordering, past-chat projection | internal-only | `raiseFault`/`settle` in transactions, fake clocks. |
| prompt.test.ts | Prompt sections in order; environment, project, character, notes, skills | partly | `documents.rs` section order, project CLAUDE/AGENTS precedence, git/manual words, skill commands, Unicode 4000/8000 cuts, binary notes, HAMLET_NET and OS; `sandbox.rs` failed-tool diagnostic; `models.rs` displayed prompt equals actual request. Missing: H33 library failure. |
| protocol-doc.test.ts | Every message type is named in docs/protocol.md | internal-only | A static documentation check. |
| protocol.test.ts | Message parsing: unknown type, missing or extra field, bad id | partly | `connection.rs` answers_malformed_messages_with_faults_and_stays_open (shout, lacks text, extra field, id, doc, what, reflectEvery, not an object). Missing: the other field forms (`text:7`, `set` lacks value, `whole:12`, `set.model`, `set.allowed`, papers lacks agent, login reply). Schema equality and `applyOp` are internal. |
| question.test.ts | An `ask_agent` question survives a server restart | covered | `society.rs` carries_a_question_across_a_crash_without_asking_twice uses `holdUntil`, SIGKILL and the same home, then checks one question, answer, conversation and closed tool. The exact after-answer/pre-follow crash window requires storage injection and is internal-only. |
| reads.test.ts | Edit and read papers, charter, prompt, history over the protocol | partly | `documents.rs` papers and history; `mayor.rs` history after resume; `models.rs` sends_the_displayed_prompt_and_holds_budget_words_for_one_turn. Missing: restoring the default charter reports `own:false`. |
| realm-acceptance.test.ts | Found by vote and overturn; cross-hamlet road and move; close/restore/erase | covered | `realm.rs` founds_a_hamlet_by_vote_has_the_asker_pay_and_overturns_it, agrees_a_road_and_a_move_between_hamlets, gathers_removal_asks_and_lets_only_the_chief_close_restore_and_erase; `cli.rs` follows_the_realm_as_hamlets_close_and_come_back. |
| realm-surface.test.ts | Another hamlet's edits and reads; removed and departed refusals; CLI `--to` | partly | `realm.rs` other-hamlet papers; `cli.rs` switching and editing; `models.rs` the_chiefs_pin_wins_and_survives_a_held_request_and_restart uses mill's mayor. Missing: a departed mill agent's farewell on papers and character. |
| record-membership.test.ts | Admission names and cap, departure refusals, restore names | partly | `society.rs` refuses_agents_the_mayor_may_not_create (duplicate after tidying), protects_an_instructed_agent_then_removes_and_undoes (task protection), removes_restores_and_erases_an_agent_by_command (office cannot leave). Missing: a name clash across hamlets, an empty reason, restore under a taken name, the `mostRoads=0` restore words. |
| record-names.test.ts | `/record` and the brief fall back to names and numbers after erasure | partly | `society.rs` keeps_record_names_and_brief_numbers_after_erasure follows create/road/remove/erase, retaining scout in `/record` and `agent #1` in the brief. Missing: a second agent with the same former name; fabricated history lacking any creation is internal-only. |
| record-office.test.ts | Roads charged once and capped; office roads follow the mayor; votes hold | partly | `society.rs` lays_and_takes_up_roads_by_command (exists, office road stays, charged once), holds_decisions_during_a_vote_follows_the_new_office_and_overturns (a decision held). Missing: the `mostRoads` cap, office roads moving to a new mayor, `/unroad` of a missing road, an implied office road counted. |
| record.test.ts | Crash replay of record tools; remove refusal diagnostics | covered | `society.rs` keeps_record_names_and_brief_numbers_after_erasure checks the `remove_agent nobody` tool failure. Crash replay at an exact storage commit requires internal injection. |
| reflection.test.ts | Reflection every N, explicit, at `/new` and close; withdrawn on stop | partly | `mayor.rs` periodic/explicit reflection, reflects_owed_work_when_starting_a_new_chat, reflects_owed_work_at_an_orderly_stop, refuses_reflection_approvals_during_shutdown, rewinds_a_reflected_turn_without_undoing_files_or_spend. Missing: withdrawal on stop. The 120 s bound is observable but needs a long wait. |
| report.test.ts | Budget report wording | internal-only | Pure `report()`. Its summary appears in the state, asserted in `documents.rs` edits_limits_one_key_or_whole_and_refuses_bad_values. |
| review-wave2.test.ts | Typed replies, held words, scrubbed refusal, stop during check, previews | partly | `mayor.rs` answers_typed_commands_that_change_nothing, keeps_words_from_a_waiting_mayor_and_answers_by_command; `documents.rs` refuses_requests_for_hamlets_and_agents_that_do_not_exist (unknown hamlet). Missing: a refusal secret scrubbed, stop during a pending check, the binary overwrite preview, a spoken credential redacted in `saying`, typed commands to an unknown hamlet. |
| rewind.test.ts | Rewind forks, keeps spend, lapses waits, refusals | partly | `mayor.rs` rewinds_a_chief_line_and_what_followed, rewinds_a_reflected_turn_without_undoing_files_or_spend (first line, retry refused, spend/files kept, empty fork after restart). Missing: running-turn/non-chief refusals and waits of another conversation. |
| roads.test.ts | Chief roads, office road refusals, `build_road` budget refusals | partly | `society.rs` lays_and_takes_up_roads_by_command, asks_the_mayor_for_a_road_and_the_mayor_builds_it. Missing: `build_road` refusals (cannot pay, same agent, office road) and simultaneous roads against one budget. |
| roles.test.ts | Tools offered by role; reserved `create_agent` waits | partly | `society.rs` asks_before_a_reserved_creation_and_caps_the_hamlet (reserved wait advertises allow/deny; only allow is exercised). Missing: denial of a reserved creation and the case where only `buildRoads` is reserved. Tool offers and ballot confinement are internal. |
| sandbox-files.test.ts | File methods never escape through a swapped parent link | internal-only | Direct `SandboxEnv` calls with a racing swapper. |
| sandbox-limits.test.ts | Exit codes, signals, ulimits, timeouts, probe messages | partly | `mayor.rs` exit 7; `sandbox.rs` bounds_commands_and_kills_timed_out_children checks signal 137, CPU/process/file/core ulimits and no late write by a timed-out child. Probe texts need a host without bubblewrap; policy hashes are internal. |
| sandbox-root-identity.test.ts | Sandbox roots cannot be replaced, moved or linked | internal-only | Direct `SandboxEnv`/`openPolicy` and private root kinds. |
| sandbox-roots.test.ts | Linked roots refuse commands; linked `houses/home` stops startup | covered | `sandbox.rs` refuses_a_work_root_replaced_by_a_link_or_removed (bash and file tools), refuses_a_linked_houses_root_at_startup (outside canary unchanged). Direct private tree-worker calls are internal. |
| sandbox.test.ts | Sandbox reach for writes, reads, network; git files; symlinks | partly | `sandbox.rs` closed/open reach matrix, outside secrets, home credentials, library read-only, own/temp writes, symlink write refusal, approved-only loopback network and git files. Missing: exact sandbox-hint words and reads through every symlink/root variant. |
| scrub.test.ts | Scrubber replaces secrets across chunks | internal-only | Pure scrubber. |
| search.test.ts | `search` and `find` results and refusals | partly | `mayor.rs` tells_what_the_mayor_is_doing exercises both tools' activity. Result contents and refusal words remain unasserted; they can be read at the loopback model boundary, even though ordinary tool output is absent from protocol v0. |
| server.test.ts | Served home: streaming equals read-back, late joiner, queue, same id | partly | `mayor.rs` answers_a_line_said_to_the_mayor, streams_a_turn_whose_ops_equal_the_transcript_read_again, admits_a_line_sent_twice_with_one_id_once; `documents.rs` edits_settings_and_refuses_bad_ones (a fresh state for a late connection). Missing: a late joiner mid-turn, a turn that finishes after every client has gone, quick lines answered in order, a write's change in the house, a resent id on a new connection. |
| skills.test.ts | Skill front matter parsing; shelf refresh | partly | `gifts.rs` copies_another_agents_whole_skill (a written SKILL.md listed in `skills`); `society.rs` shelves_an_offered_skill_for_every_agent_and_unshelves_it (description from front matter). Missing: a skill without a name skipped, first-line fields, the `---` end, refresh after a rewrite. Speed and swap races are internal. |
| society-acceptance.test.ts | Letters, roads, protection and undo, merge and split, votes, library, objections | partly | `society.rs` carries_a_letter_and_its_reply_then_asks_a_question, asks_the_mayor_for_a_road_and_the_mayor_builds_it, protects_an_instructed_agent_then_removes_and_undoes, merges_an_agent_redirects_its_letters_and_splits_it_back_out, holds_decisions_during_a_vote_follows_the_new_office_and_overturns, shelves_an_offered_skill_for_every_agent_and_unshelves_it, holds_an_instruction_for_an_objection_then_goes_or_withdraws. Missing: the held-decision explanation the TS test reads from the harness transcript. TS returns that as ordinary tool-result text, not a protocol `failure`; only the refusal's effect is asserted here. Checking the words needs inspection of the next model request. |
| society-state.test.ts | State shows hamlets, roads, departed agents, closed hamlets, library | partly | `realm.rs` closing; `society.rs` merges, letters and a_failed_peer_turn_does_not_fault_the_agent (peer error visible without chief fault/wait); `gifts.rs` starters. Missing: exact starter names, the office road between mayors and live `busy` with conversations. |
| spine.test.ts | Founding, name rules, `/to`, roads, letters, departures | partly | `realm.rs` founds_names_and_places_hamlets_by_command; `society.rs` refuses_agents_the_mayor_may_not_create (name tidied), lays_and_takes_up_roads_by_command (`/road nobody`), removes_restores_and_erases_an_agent_by_command (farewell to a departed agent); `mayor.rs` answers_a_question_the_mayor_asks. Missing: `/to scout explore`, the other name rules, `librarian/NOTES.md` on disk, a road charged to the task. Owed-instruction windows are internal. |
| state.test.ts | Deeds list, wait chat pinning, budget, activity, skill cache | partly | `mayor.rs` tells_what_the_mayor_is_doing (activity); `connection.rs` sends_every_connection_the_realms_changes (revisions increase). Missing: the exact `deeds` list, a wait pinned to a past chat after `/new`, the initial budget fields, `providers`, the think/fetch activity sequence, a new skill listed after the cache. |
| stop.test.ts | Stop drops queued lines, lapses waits, spares the next line | covered | `mayor.rs` stopping_drops_queued_lines_and_leaves_the_next_line_alone queues before the approval is raised, checks the dropped-line notice, stopped wait and next answer. The exact crash before admission needs internal injection. |
| structure.test.ts | Source import graph rules | internal-only | A static module check. |
| talk.test.ts | Chief queue, budget stop, retry, `/new`/`/resume`, trust reset on resume | partly | `mayor.rs` retry and budget scenarios, next line after repeated failure, edit trust reset on resume, queued line dropped on stop. Missing: arrival order of completed queued lines, history titles cut at 90 and another chief's session. |
| tools.test.ts | Tool behaviour: bash exits, hints, folder gone, path placement, scrubbing | partly | `mayor.rs` exits and manual approvals; `sandbox.rs` missing/replaced folder, house notes/skills versus project/git files, refused writes, non-UTF-8 edit unchanged and real timeout cleanup. Missing: sandbox hint wording, tool-output scrubbing and offered tool definitions. |
| turn.test.ts | One round's tool calls run one at a time, in order | covered | `mayor.rs` runs_one_rounds_tool_calls_in_order (a slow first call finishes before the second starts). The hook half is internal. |
| turns.test.ts | Streaming equals read-back; rewind/new reset; paging; last activity; votes | partly | `mayor.rs` streaming, tools, paging, new/rewind, reflected transcript and last error timestamp; `models.rs` usage calls; `society.rs` held votes. Missing: credentials across streamed chunks, last activity for a tool and `/new` in the office chat. |
| usage.test.ts | Conversation usage summary wording | partly | `models.rs` reads_fixed_usage_and_preserves_an_explicit_zero_bill checks no calls, `18.2k tok, $0.42`, streamed/read-back usage and explicit zero billing. Missing: million-token and fixed model-time wording boundaries (time requires a real wait or injected clock). |
| vote-words.test.ts | Tally and vote report wording | internal-only | Pure wording. "Passed:" is asserted in `society.rs` holds_decisions_during_a_vote_follows_the_new_office_and_overturns. |
| votes.test.ts | Vote rules, passing, overturn, refusals, grounding, ballots, resume | partly | `society.rs` holds_decisions_during_a_vote_follows_the_new_office_and_overturns, refuses_votes_that_cannot_be_held; `realm.rs` founds_a_hamlet_by_vote_has_the_asker_pay_and_overturns_it. Missing: silence is not a yes, grounding and sealing, three requests per ballot, stopping an open vote, one vote at a time, a recent failed vote, resume after a restart, the first ballot only. Ballot timeouts need minutes. |
| waits.test.ts | Wait joined across chats, kept across restart, lapses, faults asked again | partly | `mayor.rs` same wait/since across crash, allow/stopped closes and repeated fault `asked:2`. Missing: rewound wait and joining from another chief's chat. |
| watch-failure.test.ts | Watch and page refusals; a departed agent's chat faults with farewell | partly | `documents.rs` refuses_requests_for_hamlets_and_agents_that_do_not_exist (past 99, unknown item); `society.rs` carries_a_letter_and_its_reply_then_asks_a_question (read-only peer), removes_restores_and_erases_an_agent_by_command (current chat of a departed agent). Missing: the past and peer forms of a departed agent's chat. |
| window.test.ts | `hamlet dev` and `hamlet window`: signals, app lookup, home safety | not yet | Needs a fake `HAMLET_APP`. No scenario runs `hamlet dev` or `hamlet window`. |
| words.test.ts | Wait wording: clip, says, line, strip, waited | internal-only | Pure wording. The `says` and strip texts are asserted in `mayor.rs` keeps_words_from_a_waiting_mayor_and_answers_by_command and holds_a_command_and_an_edit_for_approval_in_manual_mode. |
| work.test.ts | `/folder` and `/mode` replies, earlier folders, restart, refusals | partly | `documents.rs` points_the_hamlet_at_a_folder_and_switches_its_mode. Missing: the folder and mode kept across a restart, a repeated `/folder` changing nothing, `/folder ~`, refusals for a child of, parent of or link to the home. |
| workspace.live.test.ts | Remote GKE workspace tools end to end | not yet | Needs a remote workspace (opt-in `HAMLET_WORKSPACE_GKE_URL`). |
| workspace.test.ts | Settings workspace choice; move-houses; remote manager and worker | partly | `documents.rs` chooses_a_remote_workspace_in_settings (move-houses refusal, http and short-token refusals, saved, token never sent, back to local). Missing: a loopback `http` address accepted, the home reopened in that workspace. The manager and worker tests need a remote workspace. |

### Counts

| Status | Files |
|---|---|
| covered | 14 |
| partly | 71 |
| not yet | 5 |
| internal-only | 21 |
| total | 111 |

### What is left uncovered, and why

- **Observable cases still missing.** The table is the full inventory, not a claim that step 4 is exhaustive. Remaining mayor cases include gate asking limits and previews, reflection withdrawal, model override restoration, offline listing refresh failure, and more hand-tool policies and scrubbed output. The current harness can exercise these; they are not internal-only.
- **Clock and long-wait cases.** Daily/monthly spend rollover, positive multi-day decision ageing, exact model-time summary wording, the 90 s hello timeout and the 120 s reflection bound need real waits or a public test clock. The suite does not add private clock controls.
- **A second chief is needed.** One token and a fixed `HAMLET_USER` give one chief. Affects foreign-hamlet refusals (chief-access, foundation-deeds, founding), joined waits (waits) and per-chief chats (chats, commands, talk). foundation-chief needs restarts under other user names.
- **An OAuth fake is needed.** ChatGPT/subscription sign-in, codes and scrubbed failures (logins, logins-connection, models-cli).
- **A pty is needed.** The tty prompt, Ctrl-D, the terminal editor and hidden key input (chat, documents-cli, models-cli).
- **Other environments.**
  - A fake `HAMLET_APP` window for bare `hamlet`, `stop`, `status`, `dev` and `window` (one-command, window).
  - A remote workspace (workspace.live, workspace).
  - A live Ollama model (ollama.live).
  - A host without bubblewrap for unavailable-sandbox probe messages.
- **Internal-only assertions.** Exact commit-boundary crashes (record replay and the after-answer question window), private loader-slot replacement, ledger storage shape and direct sandbox policy/helper races cannot be driven or distinguished through protocol v0, the CLI or workspace files. Their public consequences are checked where possible; the suite does not import server modules or inspect its database.
- **Ordinary tool output is absent from protocol v0.** It can still be checked through files or the next request to the loopback model. Missing search, letter and librarian result wording is not reclassified as internal-only just because it is absent from chat items.
