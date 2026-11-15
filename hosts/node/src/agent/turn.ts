/**
 * The rules every part of the agent shares, each written once here:
 *
 * - what an extension is built from (`Peers`);
 * - how an input's entry is written and read, so the turn's task has one
 *   writer and one reader;
 * - the stamp of a call out: its id, its conversation and the turn's task,
 *   which only `callOut` writes;
 * - how long a call out waits, by peer;
 * - what the model reads when a call is refused, fails or has an unknown
 *   outcome.
 *
 * The parts are pi-durable extensions written against pi-durable's own
 * `ToolRegistration` and `api`. This file is the only code between them.
 */
import { randomUUID } from "node:crypto";
import type { Context } from "@earendil-works/chord/context";
import {
    type ConversationId,
    type Harness,
    type InputSubmissionDraft,
    LiveDoc,
    ProviderDoc,
    type TaskId,
    type ToolExecutionApi,
    type ToolExecutionResult,
    type UserInput,
} from "@earendil-works/pi-durable";
import type {
    ActionAnswer,
    ActionArgs,
    ActionName,
    Address,
    Calls,
} from "../actor/actor/mod.ts";
import type { AgentFilesystem } from "./filesystem.ts";
import {
    type CallId,
    type ConversationRef,
    type LetterId,
    REALM,
    type RealmActions,
    type RealmTask,
    type SocietyActs,
    type SocietyTool,
    type ToolList,
    WORKSPACE,
    type WorkspaceActions,
} from "./messages.ts";

// ---------------------------------------------------------------------------
// What an extension is built from
// ---------------------------------------------------------------------------

/** The key of `Peers`' raw `Calls`. */
const CALLS: unique symbol = Symbol("hamlet.agent.calls");

/**
 * The agent's peers, which each part's factory closes over. The actor builds
 * one at each wake with `peersOf`, so a part never looks its agent up.
 */
export interface Peers {
    /**
     * The realm's three actions that are not a tool's: the gate's question
     * and charge, and the reply to a question. A tool calls out through
     * `callOut`, which this does not offer.
     */
    readonly reach: BareRealm;
    /** The agent's filesystem. */
    readonly files: AgentFilesystem;
    /** The input each conversation's run works on. */
    readonly turns: Turns;
    /** Logs a failure that leaves the turn as it is, such as a charge the realm did not take. */
    report(what: string, error: unknown): void;
    /** The raw `Calls`, which only `callOut` and `listTools` use. */
    readonly [CALLS]: Calls;
}

/** The realm's actions that are not a tool's. None carries a tool's stamp: a charge's id is the gate's own. */
export type BareRealm = {
    readonly [N in "may_spend" | "charge" | "reply"]: (
        message: Parameters<RealmActions[N]>[0],
    ) => Promise<ReturnType<RealmActions[N]>>;
};

/** The peers of one wake. */
export function peersOf(given: {
    readonly calls: Calls;
    readonly files: AgentFilesystem;
    readonly turns: Turns;
    readonly report: Peers["report"];
}): Peers {
    const reach = reaching(given.calls);
    return {
        reach: {
            may_spend: (message) => reach(REALM, "may_spend", [message]),
            charge: (message) => reach(REALM, "charge", [message]),
            reply: (message) => reach(REALM, "reply", [message]),
        },
        files: given.files,
        turns: given.turns,
        report: given.report,
        [CALLS]: given.calls,
    };
}

// ---------------------------------------------------------------------------
// The input a turn works on
// ---------------------------------------------------------------------------

/** The kinds of input an agent takes. Its entry's kind is `hamlet.<kind>`. */
export type InputKind =
    | "line"
    | "letter"
    | "question"
    | "answer"
    | "finished"
    | "wake";

/** An input to admit: its request id, its words for the model, and what its entry keeps. */
export type Admission = {
    /** The request id. pi-durable admits it once per conversation. */
    readonly id: string;
    readonly content: UserInput;
    /** The realm's task the turn is charged to. Absent: the standing task pays. */
    readonly task?: RealmTask;
} & (
    | { readonly kind: Exclude<InputKind, "question"> }
    | {
          readonly kind: "question";
          /** The letter the turn's answer replies to. */
          readonly question: LetterId;
      }
);

/** The task a turn is charged to, and where it came from. */
export type TurnTask = {
    readonly from: "input" | "standing";
    readonly task: RealmTask;
};

/** The input a conversation's run works on: the run's first input. */
export interface TurnInput {
    /** Absent when the run has no input of the agent's, such as a compaction. */
    readonly kind?: InputKind;
    readonly task: TurnTask;
    /** Set for a question. */
    readonly question?: LetterId;
}

/** An input's entry is of the kind `hamlet.<kind>`. */
const KIND = "hamlet.";

/** What an input's entry keeps. Only `draftOf` writes it and only `turnsOf` reads it. */
type Kept = { task?: RealmTask; question?: LetterId };

/**
 * The submission that admits `admission`. Its entry is
 * `{kind: "hamlet.<kind>", data: {task?, question?}}`, admitted in the same
 * commit as the input. `submit`, `finished` and the timer all admit through
 * this, so `Turns` reads back what one function wrote.
 *
 * P6's seam: steering sets `whenBusy: "steer"` here.
 */
export function draftOf(admission: Admission): InputSubmissionDraft {
    const kept: Kept = {
        ...(admission.task === undefined ? {} : { task: admission.task }),
        ...(admission.kind === "question"
            ? { question: admission.question }
            : {}),
    };
    return {
        type: "input",
        content: admission.content,
        requestId: admission.id,
        entry: { kind: `${KIND}${admission.kind}`, data: kept },
    };
}

/**
 * Runs `use` with the harness open now. A reopen swaps the harness, so
 * nothing keeps one. It is called from inside the harness's own work, by
 * tools, hooks and the gate, so it never recovers a harness itself: a
 * recovery closes the harness, and a close waits for that work.
 *
 * It throws `Closing` once the agent's harness has started to close.
 */
export type WithHarness = <T>(
    use: (harness: Harness, context: Context) => Promise<T>,
) => Promise<T>;

/**
 * The agent's harness is closing, so the run that asked is about to be
 * aborted. Whoever reads it must not turn it into a refusal for good.
 */
export class Closing extends Error {}

/** Reads the input a run works on. The gate, each tool and the reply hook all read through it. */
export interface Turns {
    of(conversation: ConversationId): Promise<TurnInput>;
    /**
     * The same, for a model request, which names only its provider session.
     * A request with no session, or one of no conversation, is charged to
     * the standing task.
     */
    ofSession(sessionId: string | undefined): Promise<TurnInput>;
}

export function turnsOf(withHarness: WithHarness, standing: RealmTask): Turns {
    // A conversation's session id is in its ProviderDoc. A miss reads every
    // conversation's once, so a conversation added later is found without a
    // step at its creation.
    const none: TurnInput = { task: { from: "standing", task: standing } };
    const sessions = new Map<string, ConversationId>();
    const of = (conversation: ConversationId): Promise<TurnInput> =>
        withHarness(async (harness, context) => {
            const live = await harness.snapshot(LiveDoc, conversation, context);
            const first = live?.run?.inputs[0];
            if (first === undefined) return none;
            const submission = await harness.submission(first, context);
            const record = await submission?.status(context);
            if (record?.type !== "input" || record.entry === undefined) {
                return none;
            }
            const handle = await harness.conversation(conversation, context);
            const page = await handle?.entries(
                { minEntryId: record.entry, maxEntryId: record.entry },
                1,
                undefined,
                context,
            );
            const entry = page?.items[0];
            if (entry === undefined || !entry.kind.startsWith(KIND)) {
                return none;
            }
            const kept = (entry.data ?? {}) as Kept;
            return {
                kind: entry.kind.slice(KIND.length) as InputKind,
                task:
                    kept.task === undefined
                        ? none.task
                        : { from: "input", task: kept.task },
                ...(kept.question === undefined
                    ? {}
                    : { question: kept.question }),
            };
        });
    const conversationOfSession = (
        sessionId: string,
    ): Promise<ConversationId | undefined> =>
        withHarness(async (harness, context) => {
            const known = sessions.get(sessionId);
            if (known !== undefined) return known;
            const all = await harness.commit(
                async (tx) =>
                    (await tx.scanConversations({}, 10_000)).items.map(
                        (conversation) => conversation.id,
                    ),
                context,
            );
            for (const id of all) {
                const provider = await harness.snapshot(
                    ProviderDoc,
                    id,
                    context,
                );
                if (provider !== undefined) {
                    sessions.set(provider.sessionId, id);
                }
            }
            return sessions.get(sessionId);
        });
    return {
        of,
        ofSession: async (sessionId) => {
            if (sessionId === undefined) return none;
            const conversation = await conversationOfSession(sessionId);
            return conversation === undefined ? none : of(conversation);
        },
    };
}

/**
 * The task the optional task fields of a call out carry: `realm_task` of
 * `run`, `write_file` and `edit_file`, and `task` of `tell`, `ask` and
 * `request_road`. It is the task of the run's first input, and absent when
 * that input had none, as the messages' doc comments say. Only `may_spend`
 * and `charge` fall back to the standing task.
 */
function carried(task: TurnTask): RealmTask | undefined {
    return task.from === "input" ? task.task : undefined;
}

// ---------------------------------------------------------------------------
// The id of a call out
// ---------------------------------------------------------------------------

/**
 * The id of a tool call, `t<task id>`. pi-durable keeps the tool's task
 * across a rerun after a kill or a close, so a rerun gives the same id.
 */
export function callOf(api: Pick<ToolExecutionApi, "taskId">): CallId {
    return `t${api.taskId}` as CallId;
}

/** The tool task a call id names, or `undefined` for an id of another form. */
export function taskOf(call: string): TaskId | undefined {
    return /^t[1-9][0-9]*$/.test(call)
        ? (Number(call.slice(1)) as TaskId)
        : undefined;
}

/** A fresh id for one provider request, `m<uuid>`. */
export function modelCall(): CallId {
    return `m${randomUUID()}` as CallId;
}

function refOf(conversation: ConversationId): ConversationRef {
    return String(conversation) as ConversationRef;
}

/** The conversation a message names, or `undefined` for text that is no id. */
export function conversationOf(ref: string): ConversationId | undefined {
    return /^[1-9][0-9]*$/.test(ref)
        ? (Number(ref) as ConversationId)
        : undefined;
}

// ---------------------------------------------------------------------------
// A call out
// ---------------------------------------------------------------------------

/**
 * How long a call out waits, by the kind of its peer. The workspace waits up
 * to its `answer_within` of 5 s before it answers `running`, so 15 s is that
 * plus a margin.
 */
const DEADLINE_MS: Readonly<Record<string, number>> = {
    [WORKSPACE.kind]: 15_000,
    [REALM.kind]: 10_000,
};

type Reach = <A, N extends ActionName<A>>(
    to: Address<A>,
    name: N,
    args: ActionArgs<A, N>,
) => Promise<ActionAnswer<A, N>>;

/**
 * `calls` with each peer's deadline. Nothing calls again: it rejects with the
 * `ActorError` of `Calls`, and the outcome of a call that timed out is
 * unknown.
 */
function reaching(calls: Calls): Reach {
    return (to, name, args) => {
        const deadlineMs = DEADLINE_MS[to.kind];
        if (deadlineMs === undefined) {
            throw new Error(`no deadline is set for a call to ${to.kind}`);
        }
        return calls.call(to, name, args, { deadlineMs });
    };
}

/** A message without the fields `callOut` stamps on it. */
type Unstamped<M> = Omit<M, "call" | "conversation" | "realm_task">;

/** An act as a society tool writes it: `callOut` sets `task` and `conversation`. */
export type ActOf<N extends SocietyTool> = Omit<
    SocietyActs[N]["act"],
    "task" | "conversation"
>;

/** Which workspace actions are stamped; computed from the messages, so the table cannot drift. */
type StampOf<F> = F extends () => unknown
    ? "read"
    : F extends (message: infer M) => unknown
      ? "call" extends keyof M
          ? "change"
          : "read"
      : never;

const WORKSPACE_STAMPS: {
    readonly [N in keyof WorkspaceActions]: StampOf<WorkspaceActions[N]>;
} = {
    run: "change",
    write_file: "change",
    edit_file: "change",
    read_file: "read",
    search: "read",
    find: "read",
    list_tools: "read",
};

/** Which acts carry the turn's task and the conversation; checked against the messages. */
const ACT_STAMPS: {
    readonly [N in SocietyTool]: {
        readonly task: "task" extends keyof SocietyActs[N]["act"]
            ? true
            : false;
        readonly conversation: "conversation" extends keyof SocietyActs[N]["act"]
            ? true
            : false;
    };
} = {
    found: { task: false, conversation: false },
    hire: { task: false, conversation: false },
    end: { task: false, conversation: false },
    seat: { task: false, conversation: false },
    tell: { task: true, conversation: false },
    ask: { task: true, conversation: true },
    lay_road: { task: false, conversation: false },
    take_up_road: { task: false, conversation: false },
    request_road: { task: true, conversation: false },
    set_tools: { task: false, conversation: false },
};

/** A tool's action of the workspace: every one but the list of hand tools. */
type WorkspaceTool = Exclude<keyof WorkspaceActions, "list_tools">;

/** What a tool writes for each action it may call at `A`: the message without its stamp. */
type MessagesOf<A> = A extends WorkspaceActions
    ? {
          readonly [N in WorkspaceTool]: WorkspaceActions[N] extends (
              message: infer M,
          ) => unknown
              ? "call" extends keyof M
                  ? Unstamped<M>
                  : M
              : never;
      }
    : A extends RealmActions
      ? { readonly [N in SocietyTool]: ActOf<N> }
      : never;

/** The answer of each action a tool may call at `A`. */
type AnswersOf<A> = A extends WorkspaceActions
    ? { readonly [N in WorkspaceTool]: ReturnType<WorkspaceActions[N]> }
    : A extends RealmActions
      ? { readonly [N in SocietyTool]: SocietyActs[N]["answer"] }
      : never;

/** An answer with its `refused` arm taken out. */
export type Granted<T> = Exclude<T, { readonly type: "refused" }>;

/**
 * A tool's call out, as the tool's result. The tool writes the message
 * without its stamp, and this adds it:
 *
 * - a change of the workspace gets `call: t<task id>`, `conversation` and
 *   `realm_task`; a society tool's act is sent as `Tool{call, act}`, and the
 *   act gets `conversation` and `task` where it has them. A read gets none,
 *   and reads no turn for it;
 * - `refused` gives an error result holding the taker's words;
 * - a call that fails gives an error result that says so. A stamped call's
 *   outcome is unknown, and the words say that;
 * - any other answer goes to `said`, which no longer sees `refused`.
 *
 * It never calls twice. It throws only when `context` is aborted while the
 * call is out, which is how a close cuts a tool off: pi-durable then reruns
 * the tool at the next wake, and the rerun sends the same id.
 */
export async function callOut<
    A,
    N extends keyof MessagesOf<A> & keyof AnswersOf<A> & string,
>(
    peers: Peers,
    api: Pick<ToolExecutionApi, "taskId" | "conversationId">,
    context: Context,
    to: Address<A>,
    name: N,
    message: MessagesOf<A>[N],
    said: (
        answer: Granted<AnswersOf<A>[N]>,
    ) => ToolExecutionResult | Promise<ToolExecutionResult>,
): Promise<ToolExecutionResult> {
    const stamp = await stamped(peers, api, to, name, message as object);
    let answer: unknown;
    try {
        answer = await cutOff(
            context.abortSignal,
            reaching(peers[CALLS])(
                to as Address,
                name as never,
                [stamp.message] as never,
            ),
        );
    } catch (error) {
        if (context.abortSignal?.aborted) throw error;
        return stamp.changes
            ? unknownOutcome(`\`${name}\``, reasonOf(error))
            : failed(
                  `\`${name}\` failed: ${reasonOf(error)}. Nothing was changed.`,
              );
    }
    if (isRefused(answer)) return failed(answer.words);
    return said(answer as Granted<AnswersOf<A>[N]>);
}

/** `message` with the stamp its action carries, and whether it carries one. */
async function stamped(
    peers: Peers,
    api: Pick<ToolExecutionApi, "taskId" | "conversationId">,
    to: Address<unknown>,
    name: string,
    message: object,
): Promise<{ readonly message: object; readonly changes: boolean }> {
    const call = callOf(api);
    const conversation = refOf(api.conversationId);
    const task = async () =>
        carried((await peers.turns.of(api.conversationId)).task);
    if (to.kind === WORKSPACE.kind) {
        if (WORKSPACE_STAMPS[name as WorkspaceTool] !== "change") {
            return { message, changes: false };
        }
        const realmTask = await task();
        return {
            message: {
                ...message,
                call,
                conversation,
                ...(realmTask === undefined ? {} : { realm_task: realmTask }),
            },
            changes: true,
        };
    }
    const wants = ACT_STAMPS[name as SocietyTool];
    const actTask = wants.task ? await task() : undefined;
    return {
        message: {
            call,
            act: {
                ...message,
                ...(wants.conversation ? { conversation } : {}),
                ...(actTask === undefined ? {} : { task: actTask }),
            },
        },
        changes: true,
    };
}

/**
 * The tools a chief added by hand, as the workspace lists them. The one call
 * out that is neither a tool's nor the realm's.
 */
export function listTools(peers: Peers): Promise<ToolList> {
    return reaching(peers[CALLS])(WORKSPACE, "list_tools", []);
}

/**
 * `work`, or a rejection as soon as `signal` aborts. The work itself goes
 * on: a call that is out stays out, with its outcome unknown to this side.
 */
export function cutOff<T>(
    signal: AbortSignal | undefined,
    work: Promise<T>,
): Promise<T> {
    if (signal === undefined) return work;
    const { promise, reject } = Promise.withResolvers<never>();
    const abort = () => reject(signal.reason ?? new Error("cut off"));
    if (signal.aborted) abort();
    else signal.addEventListener("abort", abort, { once: true });
    work.catch(() => {});
    return Promise.race([work, promise]).finally(() =>
        signal.removeEventListener("abort", abort),
    );
}

/** A tool's result of plain words. */
export function words(text: string): ToolExecutionResult {
    return { output: [{ type: "text", text }] };
}

/** A tool's error result of plain words. */
export function failed(text: string): ToolExecutionResult {
    return { output: [{ type: "text", text }], isError: true };
}

/**
 * The error result of work whose outcome nobody knows: a call out that
 * failed, or an own edit a kill cut off. The one place these words are
 * written.
 */
export function unknownOutcome(what: string, why: string): ToolExecutionResult {
    return failed(
        `${what} was cut off: ${why}. Its outcome is unknown: it may have been carried out or not, and nothing sends it again. Look at what it would have changed before you repeat it.`,
    );
}

/** An error in a few words: an `ActorError`'s `group.code`, then its message. */
export function reasonOf(error: unknown): string {
    if (typeof error === "object" && error !== null) {
        const { group, code, message } = error as {
            group?: unknown;
            code?: unknown;
            message?: unknown;
        };
        if (typeof group === "string" && typeof code === "string") {
            return `${group}.${code}: ${String(message)}`;
        }
        if (typeof message === "string") return message;
    }
    return String(error);
}

function isRefused(answer: unknown): answer is { words: string } {
    return (
        typeof answer === "object" &&
        answer !== null &&
        (answer as { type?: unknown }).type === "refused"
    );
}
