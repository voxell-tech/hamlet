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
    type ReadAnswer,
    type ReadFile,
    type RealmActions,
    type RealmTask,
    type SocietyActs,
    type SocietyTool,
    type ToolList,
    WORKSPACE,
    type WorkspaceActions,
} from "./messages.ts";

const CALLS: unique symbol = Symbol("hamlet.agent.calls");

export interface Peers {
    readonly reach: BareRealm;
    readonly cancel: (
        message: Parameters<WorkspaceActions["cancel"]>[0],
    ) => Promise<ReturnType<WorkspaceActions["cancel"]>>;
    readonly files: AgentFilesystem;
    readonly turns: Turns;
    report(what: string, error: unknown): void;
    readonly [CALLS]: Calls;
}

/** The realm's actions an agent calls outside a tool. */
const BARE = [
    "may_spend",
    "charge",
    "reply",
    "credential",
    "keep_credential",
    "turn_failed",
    "lapse",
    "open_task",
    "read_copies",
    "worker_done",
    "errand_done",
] as const;

export type BareRealm = {
    readonly [N in (typeof BARE)[number]]: (
        message: Parameters<RealmActions[N]>[0],
    ) => Promise<ReturnType<RealmActions[N]>>;
};

export function peersOf(given: {
    readonly calls: Calls;
    readonly files: AgentFilesystem;
    readonly turns: Turns;
    readonly report: Peers["report"];
}): Peers {
    const reach = reaching(given.calls);
    return {
        reach: Object.fromEntries(
            BARE.map((name) => [
                name,
                (message: never) => reach(REALM, name, [message]),
            ]),
        ) as unknown as BareRealm,
        cancel: (message) => reach(WORKSPACE, "cancel", [message]),
        files: given.files,
        turns: given.turns,
        report: given.report,
        [CALLS]: given.calls,
    };
}

export type InputKind =
    | "line"
    | "letter"
    | "question"
    | "answer"
    | "finished"
    | "wake"
    | "retry"
    | "reflection"
    | "cut-off";

export type Admission = {
    /** pi-durable admits it once per conversation. */
    readonly id: string;
    readonly content: UserInput;
    /** The realm's task the turn is charged to. Absent: the standing task pays. */
    readonly task?: RealmTask;
} & (
    | { readonly kind: Exclude<InputKind, "question" | "cut-off"> }
    | {
          readonly kind: "question";
          readonly question: LetterId;
      }
    | {
          readonly kind: "cut-off";
          readonly question?: LetterId;
      }
);

export type TurnTask = {
    readonly from: "input" | "standing";
    readonly task: RealmTask;
};

export interface TurnInput {
    /** Absent when the run has no input of the agent's, such as a compaction. */
    readonly kind?: InputKind;
    readonly task: TurnTask;
    readonly question?: LetterId;
}

const KIND = "hamlet.";

type Kept = { task?: RealmTask; question?: LetterId };

/**
 * Its entry is admitted in the same commit as the input. A busy run takes a
 * `steer` input into its turn; any other waits for the next run.
 */
export function draftOf(
    admission: Admission,
    whenBusy?: "steer",
): InputSubmissionDraft {
    const kept: Kept = {
        ...(admission.task === undefined ? {} : { task: admission.task }),
        ...("question" in admission && admission.question !== undefined
            ? { question: admission.question }
            : {}),
    };
    return {
        type: "input",
        content: admission.content,
        requestId: admission.id,
        ...(whenBusy === undefined ? {} : { whenBusy }),
        entry: { kind: `${KIND}${admission.kind}`, data: kept },
    };
}

/**
 * A reopen swaps the harness, so nothing keeps one. It throws `Closing` once
 * the agent's harness has started to close.
 */
export type WithHarness = <T>(
    use: (harness: Harness, context: Context) => Promise<T>,
) => Promise<T>;

/**
 * Whoever reads it must not turn it into a refusal for good: the run is
 * about to be aborted.
 */
export class Closing extends Error {}

export interface Turns {
    of(conversation: ConversationId): Promise<TurnInput>;
    ofSession(sessionId: string | undefined): Promise<TurnInput>;
}

export function turnsOf(withHarness: WithHarness, standing: RealmTask): Turns {
    // A miss reads every conversation's ProviderDoc once, so a conversation
    // added later is found without a step at its creation.
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
 * Absent when the run's first input had none: only `may_spend` and `charge`
 * fall back to the standing task.
 */
function carried(task: TurnTask): RealmTask | undefined {
    return task.from === "input" ? task.task : undefined;
}

/**
 * pi-durable keeps the tool's task across a rerun after a kill or a close, so
 * a rerun gives the same id.
 */
export function callOf(api: Pick<ToolExecutionApi, "taskId">): CallId {
    return `t${api.taskId}` as CallId;
}

export function taskOf(call: string): TaskId | undefined {
    return /^t[1-9][0-9]*$/.test(call)
        ? (Number(call.slice(1)) as TaskId)
        : undefined;
}

export function modelCall(): CallId {
    return `m${randomUUID()}` as CallId;
}

export function refOf(conversation: ConversationId): ConversationRef {
    return String(conversation) as ConversationRef;
}

export function conversationOf(ref: string): ConversationId | undefined {
    return /^[1-9][0-9]*$/.test(ref)
        ? (Number(ref) as ConversationId)
        : undefined;
}

/**
 * The workspace waits up to its `answer_within` of 5 s before it answers
 * `running`, so 15 s is that plus a margin.
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

function reaching(calls: Calls): Reach {
    return (to, name, args) => {
        const deadlineMs = DEADLINE_MS[to.kind];
        if (deadlineMs === undefined) {
            throw new Error(`no deadline is set for a call to ${to.kind}`);
        }
        return calls.call(to, name, args, { deadlineMs });
    };
}

type Unstamped<M> = Omit<M, "call" | "conversation" | "realm_task">;

export type ActOf<N extends SocietyTool> = Omit<
    SocietyActs[N]["act"],
    "task" | "conversation"
>;

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
    cancel: "read",
};

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

type WorkspaceTool = Exclude<keyof WorkspaceActions, "list_tools" | "cancel">;

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

type AnswersOf<A> = A extends WorkspaceActions
    ? { readonly [N in WorkspaceTool]: ReturnType<WorkspaceActions[N]> }
    : A extends RealmActions
      ? { readonly [N in SocietyTool]: SocietyActs[N]["answer"] }
      : never;

export type Granted<T> = Exclude<T, { readonly type: "refused" }>;

/**
 * It never calls twice. It throws only when `context` is aborted while the
 * call is out, which is how a close cuts a tool off: pi-durable then reruns
 * the tool at the next wake, and the rerun sends the same id. `tool` is the
 * name the model called.
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
    tool: string,
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
            ? interrupted(tool)
            : failed(
                  `\`${name}\` failed: ${reasonOf(error)}. Nothing was changed.`,
              );
    }
    if (isRefused(answer)) return failed(answer.words);
    return said(answer as Granted<AnswersOf<A>[N]>);
}

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

export function listTools(peers: Peers): Promise<ToolList> {
    return reaching(peers[CALLS])(WORKSPACE, "list_tools", []);
}

export function readFile(peers: Peers, message: ReadFile): Promise<ReadAnswer> {
    return reaching(peers[CALLS])(WORKSPACE, "read_file", [message]);
}

/** The work itself goes on: a call that is out stays out. */
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

export function words(text: string): ToolExecutionResult {
    return { output: [{ type: "text", text }] };
}

export function failed(text: string): ToolExecutionResult {
    return { output: [{ type: "text", text }], isError: true };
}

/** A call that threw, as today's harness gives it: no text and one error. */
export function thrown(code: string, message: string): ToolExecutionResult {
    return {
        output: [{ type: "text", text: "" }],
        isError: true,
        diagnostics: [{ severity: "error", code, message }],
    };
}

/** Today's words for a call that may have run (`crates/hamlet_server/src/calls.rs`). */
export function interrupted(tool: string): ToolExecutionResult {
    return thrown(
        "tool_interrupted",
        `Tool ${tool} was interrupted by a restart and may have partially run`,
    );
}

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
