import { randomUUID } from "node:crypto";
import type { Context } from "@earendil-works/chord/context";
import type { ToolResultMessage } from "@earendil-works/pi-ai";
import {
    type ConversationId,
    type Harness,
    InboxDoc,
    type InboxItem,
    type InboxState,
    type InputSubmissionDraft,
    LiveDoc,
    type LiveState,
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
import type { Author, ChatFile, ChatImage } from "./messages/agent.ts";
import {
    LIBRARY,
    type Offer,
    type OfferAnswer,
    type ShelfList,
} from "./messages/library.ts";
import {
    type Files,
    type FilesAnswer,
    type LearnedList,
    profileAt,
    readLearned,
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
    readonly fitImage: (
        message: Parameters<WorkspaceActions["fit_image"]>[0],
    ) => Promise<ReturnType<WorkspaceActions["fit_image"]>>;
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
    "working",
    "open_task",
    "read_copies",
    "secrets",
    "worker_done",
    "errand_done",
    "read_place",
    "read_own_place",
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
        fitImage: (message) => reach(WORKSPACE, "fit_image", [message]),
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

export type CallEnd =
    | {
          readonly type: "ran";
          readonly exit: number;
          readonly ms?: number;
          readonly failure?: string;
          readonly created?: boolean;
      }
    | { readonly type: "blocked"; readonly why: string }
    | { readonly type: "aborted"; readonly withdrawn?: true };

export type Ended = { readonly call: string; readonly end: CallEnd };

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
    readonly ended?: Ended;
    /** The images and files of a chief's line, as its item shows them. */
    readonly images?: readonly ChatImage[];
    readonly files?: readonly ChatFile[];
    /** A chief's line's words, where its content holds captions besides. */
    readonly text?: string;
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

export const KIND = "hamlet.";

export type Kept = {
    task?: RealmTask;
    question?: LetterId;
    /** The compiler takes no interface as an entry's JSON data; a `Pick` of one it does. */
    from?: Pick<Author, keyof Author>;
    note?: string;
    chief?: string;
    ended?: Ended;
    images?: Pick<ChatImage, keyof ChatImage>[];
    files?: Pick<ChatFile, keyof ChatFile>[];
    /** The words an item shows, where the model's content says more. */
    text?: string;
};

/**
 * Its entry is admitted in the same commit as the input. A busy run takes a
 * `steer` input into its turn; any other waits for the next run.
 */
export function draftOf(
    admission: Admission,
    whenBusy?: "steer",
): InputSubmissionDraft {
    return {
        type: "input",
        content: admission.content,
        requestId: admission.id,
        ...(whenBusy === undefined ? {} : { whenBusy }),
        entry: { kind: `${KIND}${admission.kind}`, data: keptOf(admission) },
    };
}

/** What an input's entry keeps beside its words. */
export function keptOf(admission: Admission): Kept {
    return {
        ...(admission.task === undefined ? {} : { task: admission.task }),
        ...("question" in admission && admission.question !== undefined
            ? { question: admission.question }
            : {}),
        ...(admission.from === undefined ? {} : { from: admission.from }),
        ...(admission.note === undefined ? {} : { note: admission.note }),
        ...(admission.chief === undefined ? {} : { chief: admission.chief }),
        ...(admission.ended === undefined ? {} : { ended: admission.ended }),
        ...(admission.images === undefined || admission.images.length === 0
            ? {}
            : { images: [...admission.images] }),
        ...(admission.files === undefined || admission.files.length === 0
            ? {}
            : { files: [...admission.files] }),
        ...(admission.text === undefined ? {} : { text: admission.text }),
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
            let first: { kind: InputKind; kept: Kept } | undefined;
            const turnOf = (task: RealmTask | undefined): TurnInput =>
                first === undefined
                    ? none
                    : {
                          kind: first.kind,
                          task:
                              task === undefined
                                  ? none.task
                                  : { from: "input", task },
                          ...(first.kept.question === undefined
                              ? {}
                              : { question: first.kept.question }),
                      };
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
                if (!entry?.kind.startsWith(KIND)) continue;
                const kind = entry.kind.slice(KIND.length) as InputKind;
                const kept = (entry.data ?? {}) as Kept;
                first ??= { kind, kept };
                if (kind !== "finished") return turnOf(kept.task);
            }
            return turnOf(first?.kept.task);
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
 * Whether `conversation` has a run or an input queued for one: an open
 * input.
 */
export async function isOpen(
    harness: Harness,
    conversation: ConversationId,
    context: Context,
): Promise<boolean> {
    if (await isBusy(harness, conversation, context)) return true;
    const inbox = await harness.snapshot(InboxDoc, conversation, context);
    return inbox?.items.some(isQueuedInput) ?? false;
}

function isQueuedInput(item: InboxItem): boolean {
    return item.mode !== "write";
}

/**
 * A follower of harnesses that tells the realm when the agent's open inputs
 * go from none to some and back.
 */
export function workReporter(peers: Peers): (harness: Harness) => void {
    const running = new Set<ConversationId>();
    const queued = new Set<ConversationId>();
    const followed = new WeakSet<Harness>();
    let open = false;
    let told: Promise<void> = Promise.resolve();
    const mark = (set: Set<ConversationId>, id: ConversationId, on: boolean) =>
        on ? set.add(id) : set.delete(id);
    return (harness) => {
        if (followed.has(harness)) return;
        followed.add(harness);
        harness.observeCommits({
            advance: ({ changes }) => {
                for (const change of changes) {
                    if (
                        change.type !== "document" ||
                        change.conversationId === undefined
                    ) {
                        continue;
                    }
                    const id = change.conversationId;
                    const value =
                        change.kind === "change" ? change.value : undefined;
                    if (change.record.kind === LiveDoc.definition.kind) {
                        const live = value as LiveState | undefined;
                        mark(running, id, live?.run !== undefined);
                    } else if (
                        change.record.kind === InboxDoc.definition.kind
                    ) {
                        const inbox = value as InboxState | undefined;
                        mark(
                            queued,
                            id,
                            inbox?.items.some(isQueuedInput) ?? false,
                        );
                    }
                }
                const now = running.size + queued.size > 0;
                if (now === open) return;
                open = now;
                told = told
                    .then(() => peers.reach.working({ open: now }))
                    .then(
                        () => undefined,
                        (error: unknown) =>
                            peers.report(
                                "the realm was not told whether the agent works",
                                error,
                            ),
                    );
            },
            end: () => {},
        });
    };
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
    fit_image: "read",
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
    merge_hires: [false, false],
    remove_hamlet: [false, false],
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
        if (stamp.changes) {
            peers.report(`${tool} was interrupted`, error);
            return interrupted(tool);
        }
        return failed(
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
    readonly learned: () => Promise<LearnedList>;
} {
    const reach = reaching(peers[CALLS]);
    const to = profileAt(profile);
    return {
        files: (message) => reach(to, "files", [message]),
        save: (message) => reach(to, "save", [message]),
        learned: async () => readLearned(await reach(to, "learned", [])),
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
 * The workspace's tool list, read once for a wake, and read again after a
 * failed read.
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

/** A call that threw: no text and one error. */
export function thrown(code: string, message: string): ToolExecutionResult {
    return {
        output: [{ type: "text", text: "" }],
        isError: true,
        diagnostics: [{ severity: "error", code, message }],
    };
}

/** The words for a call that may have run. */
export const MAY_HAVE_RUN =
    "interrupted by a restart and may have partially run";

/**
 * The result of a held call that ended without an answer, as pi-durable's
 * abort writes it: "Tool <name> was aborted".
 */
export function abortedResult(held: ToolResultMessage): ToolResultMessage {
    return {
        role: "toolResult",
        toolCallId: held.toolCallId,
        toolName: held.toolName,
        content: [
            {
                type: "text",
                text: `<harness>\n[error] Tool ${held.toolName} was aborted\n</harness>`,
            },
        ],
        isError: true,
        timestamp: held.timestamp,
    };
}

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
