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
import type {
    Author,
    ChatFile,
    ChatImage,
    HeldCall,
} from "./messages/agent.ts";
import {
    LIBRARY,
    type Offer,
    type OfferAnswer,
    type ShelfList,
} from "./messages/library.ts";
import {
    type Files,
    type FilesAnswer,
    profileAt,
    type Save,
    type SaveAnswer,
} from "./messages/profile.ts";
import {
    REALM,
    type RealmActions,
    type SocietyActs,
    type SocietyTool,
} from "./messages/realm.ts";
import type {
    CallId,
    ConversationRef,
    LetterId,
    ProfileId,
    RealmTask,
    Refused,
} from "./messages/shared.ts";
import {
    type ReadAnswer,
    type ReadFile,
    type ToolList,
    WORKSPACE,
    type WorkspaceActions,
} from "./messages/workspace.ts";

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
    "secrets",
    "worker_done",
    "errand_done",
    "read_place",
    "fetch_part",
] as const;

export type BareRealm = {
    readonly [N in (typeof BARE)[number]]: (
        ...message: Parameters<RealmActions[N]>
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
                (...message: never) => reach(REALM, name, message),
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
    | "notice"
    | "worker-answer"
    | "worker"
    | "errand"
    | "finished"
    | "wake"
    | "retry"
    | "reflection"
    | "cut-off";

/** Between a chief's line and the note of the lead who reviewed it. */
export const NOTE_BREAK = "\n\n";

export type Admission = {
    /** pi-durable admits it once per conversation. */
    readonly id: string;
    readonly content: UserInput;
    /** The realm's task the turn is charged to. Absent: the standing task pays. */
    readonly task?: RealmTask;
    /** Who sent a letter, a question, an answer or a worker's answer. */
    readonly from?: Author;
    /** The note of the lead who reviewed a chief's line, at the end of its content. */
    readonly note?: string;
    /** The name of the chief who said a line. */
    readonly chief?: string;
    readonly held?: HeldCall;
    /** The images and files of a chief's line, as its item shows them. */
    readonly images?: readonly ChatImage[];
    readonly files?: readonly ChatFile[];
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
    /** The conversation `Turns.ofSession` found the session in. */
    readonly conversation?: ConversationId;
    /** Absent when the run has no input of the agent's, such as a compaction. */
    readonly kind?: InputKind;
    readonly task: TurnTask;
    readonly question?: LetterId;
}

const KIND = "hamlet.";

export type Kept = {
    task?: RealmTask;
    question?: LetterId;
    /** The compiler takes no interface as an entry's JSON data; a `Pick` of one it does. */
    from?: Pick<Author, keyof Author>;
    note?: string;
    chief?: string;
    held?: Pick<HeldCall, keyof HeldCall>;
    images?: Pick<ChatImage, keyof ChatImage>[];
    files?: Pick<ChatFile, keyof ChatFile>[];
};

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
        ...(admission.from === undefined ? {} : { from: admission.from }),
        ...(admission.note === undefined ? {} : { note: admission.note }),
        ...(admission.chief === undefined ? {} : { chief: admission.chief }),
        ...(admission.held === undefined ? {} : { held: admission.held }),
        ...(admission.images === undefined || admission.images.length === 0
            ? {}
            : { images: [...admission.images] }),
        ...(admission.files === undefined || admission.files.length === 0
            ? {}
            : { files: [...admission.files] }),
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
            const handle = await harness.conversation(conversation, context);
            const placed: { kind: InputKind; kept: Kept }[] = [];
            for (const input of live?.run?.inputs ?? []) {
                const submission = await harness.submission(input, context);
                const record = await submission?.status(context);
                if (record?.type !== "input" || record.entry === undefined) {
                    continue;
                }
                const page = await handle?.entries(
                    { minEntryId: record.entry, maxEntryId: record.entry },
                    1,
                    undefined,
                    context,
                );
                const entry = page?.items[0];
                if (entry?.kind.startsWith(KIND)) {
                    placed.push({
                        kind: entry.kind.slice(KIND.length) as InputKind,
                        kept: (entry.data ?? {}) as Kept,
                    });
                }
            }
            const [first] = placed;
            if (first === undefined) return none;
            const task = placed.findLast(({ kept }) => kept.task !== undefined)
                ?.kept.task;
            return {
                kind: first.kind,
                task: task === undefined ? none.task : { from: "input", task },
                ...(first.kept.question === undefined
                    ? {}
                    : { question: first.kept.question }),
            };
        });
    const conversationOfSession = (
        sessionId: string,
    ): Promise<ConversationId | undefined> =>
        withHarness(async (harness, context) => {
            const known = sessions.get(sessionId);
            if (known !== undefined) return known;
            for (const id of await conversationsOf(harness, context)) {
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
            return conversation === undefined
                ? none
                : { ...(await of(conversation)), conversation };
        },
    };
}

export async function conversationsOf(
    harness: Harness,
    context: Context,
): Promise<ConversationId[]> {
    return harness.commit(
        async (tx) =>
            (await tx.scanConversations({}, 10_000)).items.map(
                (conversation) => conversation.id,
            ),
        context,
    );
}

/** Whether a run is going on in `conversation`, its `afterRun` tasks included. */
export async function isBusy(
    harness: Harness,
    conversation: ConversationId,
    context: Context,
): Promise<boolean> {
    const live = await harness.snapshot(LiveDoc, conversation, context);
    return live?.run !== undefined;
}

/**
 * Absent when no input of the run had one: only `may_spend` and `charge`
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
    [LIBRARY.kind]: 10_000,
    profile: 10_000,
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

type Carries<
    N extends SocietyTool,
    K extends string,
> = K extends keyof SocietyActs[N]["act"]
    ? Record<string, never> extends Pick<SocietyActs[N]["act"], K>
        ? boolean
        : true
    : false;

/**
 * Whether an act carries the calling turn's conversation, and its task, in
 * itself. An act's task is the turn's, the standing one included.
 */
const OWN: {
    readonly [N in SocietyTool]: readonly [
        conversation: Carries<N, "conversation">,
        task: Carries<N, "task">,
    ];
} = {
    found: [false, false],
    hire: [false, false],
    end: [false, false],
    seat: [false, false],
    tell: [false, false],
    ask: [true, false],
    lay_road: [false, false],
    take_up_road: [false, false],
    request_road: [false, false],
    set_tools: [false, false],
    ask_chief: [true, true],
    object: [true, true],
    start_worker: [false, true],
    set_allowance: [false, false],
    read_budget: [false, false],
    agree_road: [false, false],
    set_nickname: [false, false],
    set_model: [false, false],
    read_record: [false, false],
    create_profile: [false, false],
    read_profile: [false, false],
    edit_profile: [false, false],
    tag: [false, false],
    revert: [false, false],
    ask_librarian: [false, false],
    submit_skill: [false, false],
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
    refused?: (answer: Refused) => ToolExecutionResult | undefined,
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
    if (isRefused(answer)) return refused?.(answer) ?? failed(answer.words);
    return said(answer as Granted<AnswersOf<A>[N]>);
}

async function stamped(
    peers: Peers,
    api: Pick<ToolExecutionApi, "taskId" | "conversationId">,
    to: Address<unknown>,
    name: string,
    message: object,
): Promise<{ readonly message: object; readonly changes: boolean }> {
    if (
        to.kind === WORKSPACE.kind &&
        WORKSPACE_STAMPS[name as WorkspaceTool] !== "change"
    ) {
        return { message, changes: false };
    }
    const call = callOf(api);
    const conversation = refOf(api.conversationId);
    const turn = (await peers.turns.of(api.conversationId)).task;
    const task = carried(turn);
    if (to.kind === WORKSPACE.kind) {
        return {
            message: {
                ...message,
                call,
                conversation,
                ...(task === undefined ? {} : { realm_task: task }),
            },
            changes: true,
        };
    }
    const [ownConversation, ownTask] = OWN[name as SocietyTool];
    return {
        message: {
            call,
            ...(task === undefined ? {} : { task }),
            conversation,
            act: {
                ...message,
                ...(ownConversation ? { conversation } : {}),
                ...(ownTask ? { task: turn.task } : {}),
            },
        },
        changes: true,
    };
}

/** The calls the agent makes on the profile it was filled from. */
export function profileCalls(
    peers: Peers,
    profile: ProfileId,
): {
    readonly files: (message: Files) => Promise<FilesAnswer>;
    readonly save: (message: Save) => Promise<SaveAnswer>;
} {
    const reach = reaching(peers[CALLS]);
    const to = profileAt(profile);
    return {
        files: (message) => reach(to, "files", [message]),
        save: (message) => reach(to, "save", [message]),
    };
}

export function libraryCalls(peers: Peers): {
    readonly shelves: (version_seen?: number) => Promise<ShelfList>;
    readonly offer: (message: Offer) => Promise<OfferAnswer>;
} {
    const reach = reaching(peers[CALLS]);
    return {
        shelves: (version_seen) =>
            reach(LIBRARY, "shelves", [
                version_seen === undefined ? {} : { version_seen },
            ]),
        offer: (message) => reach(LIBRARY, "offer", [message]),
    };
}

/**
 * The workspace's tool list, read once for a wake as today reads it once
 * for a server, and read again after a failed read.
 */
export function toolLister(peers: Peers): () => Promise<ToolList> {
    let listed: Promise<ToolList> | undefined;
    return () => {
        listed ??= reaching(peers[CALLS])(WORKSPACE, "list_tools", []).catch(
            (error: unknown) => {
                listed = undefined;
                throw error;
            },
        );
        return listed;
    };
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
export const MAY_HAVE_RUN =
    "interrupted by a restart and may have partially run";

export function interrupted(tool: string): ToolExecutionResult {
    return thrown("tool_interrupted", `Tool ${tool} was ${MAY_HAVE_RUN}`);
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

function isRefused(answer: unknown): answer is Refused {
    return (
        typeof answer === "object" &&
        answer !== null &&
        (answer as { type?: unknown }).type === "refused"
    );
}
