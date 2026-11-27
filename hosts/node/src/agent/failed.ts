/**
 * A turn that ended with no answer. Its words go to its conversation as a
 * notice. A turn in the chief's current session tells the realm, which
 * opens a wait for the chief, and a turn on a question replies to its asker.
 */
import type { Context } from "@earendil-works/chord/context";
import {
    type CommitChange,
    type ConversationId,
    defineExtension,
    defineTask,
    type EntryDraft,
    type EntryId,
    type EntryRecord,
    type Extension,
    GenerationTask,
    type Harness,
    hook,
    InboxDoc,
    LiveDoc,
    type LiveState,
    type RunFailure,
    type RunFailureReason,
    type SubmissionId,
    type SubmissionRecord,
    type TaskId,
    type TaskRuntime,
    type Tx,
} from "@earendil-works/pi-durable";
import { notice } from "./commands.ts";
import type { TurnFailed } from "./messages/realm.ts";
import type { LetterId, RealmTask } from "./messages/shared.ts";
import { realmWordsIn, unsentOf } from "./spend.ts";
import {
    Closing,
    cutOff,
    type Kept,
    type Peers,
    refOf,
    type WithHarness,
} from "./turn.ts";

export interface Failure {
    readonly conversation: ConversationId;
    /** Absent: the agent's standing task. */
    readonly task: RealmTask | undefined;
    readonly words: string;
    readonly cause: TurnFailed["cause"];
    readonly question: LetterId | undefined;
}

/**
 * Tells the realm of a failure in the chief's current session, or replies to
 * the question the turn was to answer; never throws.
 */
export type TellFailed = (
    failure: Failure,
    signal: AbortSignal | undefined,
) => Promise<void>;

export function tellingFailed(
    peers: Peers,
    standing: RealmTask,
    chief: () => Promise<ConversationId | undefined>,
): TellFailed {
    return async ({ conversation, task, words, cause, question }, signal) => {
        try {
            if (question !== undefined) {
                await couldNotAnswer(peers, question, words, signal);
                return;
            }
            if ((await chief()) !== conversation) return;
            await cutOff(
                signal,
                peers.reach.turn_failed({
                    task: task ?? standing,
                    conversation: refOf(conversation),
                    words,
                    cause,
                }),
            );
        } catch (error) {
            if (error instanceof Closing || signal?.aborted) return;
            peers.report("a failed turn was not told to the realm", error);
        }
    };
}

export async function couldNotAnswer(
    peers: Peers,
    question: LetterId,
    words: string,
    signal: AbortSignal | undefined,
): Promise<void> {
    const text = `It could not answer: ${words}`;
    const taken = await cutOff(signal, peers.reach.reply({ question, text }));
    if (taken.type === "refused") {
        peers.report("a reply was refused", taken.words);
    }
}

/**
 * Ends a run-end task with `words` as a notice, then tells the realm.
 * `input` is the entry of the input the run answered.
 */
export async function failTurn<I, S>(
    runtime: TaskRuntime<I, S, null, object>,
    input: EntryRecord | undefined,
    words: string,
    cause: TurnFailed["cause"],
    tell: TellFailed,
    context: Context,
): Promise<void> {
    const conversation = await runtime.conversation(
        runtime.conversationId,
        context,
    );
    await runtime.commit(async (tx) => {
        await conversation?.submit(
            tx,
            notice(`failed:${runtime.taskId}`, words),
        );
        return {
            status: "terminal",
            outcome: { status: "completed", result: null },
        };
    }, context);
    const kept = input?.data as Kept | null | undefined;
    await tell(
        {
            conversation: runtime.conversationId,
            task: kept?.task,
            words,
            cause,
            question: kept?.question,
        },
        context.abortSignal,
    );
}

const EMPTY = "the model answered with nothing. Send the message again";

type EmptyInput = { input: EntryId | null };

/**
 * C29, for an empty answer C28 does not ask again. pi-durable settled the
 * run's input answered, so it fails here.
 */
export function emptyAnswer(tell: TellFailed) {
    return defineTask<EmptyInput, { phase: "fail" }, null>({
        name: "hamlet.empty-answer",
        version: 1,
        initial: () => ({ phase: "fail" }),
        phases: {
            fail: async (task, runtime, context) => {
                const input =
                    task.input.input === null
                        ? undefined
                        : await runtime.entry(task.input.input, context);
                await failTurn(runtime, input, EMPTY, "model", tell, context);
            },
        },
        abort: (_task, runtime, context) =>
            runtime.commit(
                () => ({ status: "terminal", outcome: { status: "aborted" } }),
                context,
            ),
    });
}

/** A callback run once a turn ends, answered or not. It never rejects. */
export type TurnEnded = (conversation: ConversationId) => Promise<void>;

export interface FailedTurns {
    readonly extension: Extension;
    /** Follows every run of `harness` that fails from now on. */
    follow(harness: Harness): void;
}

/** A run a chief stopped ends this way; it failed only when nobody did. */
const STOPPED: ReadonlySet<RunFailureReason> = new Set(["aborted", "reset"]);

type Unanswered = Extract<SubmissionRecord, { status: "unanswered" }>;

interface FailedRun {
    readonly conversation: ConversationId;
    readonly failure: RunFailure;
    readonly input: Unanswered;
}

/**
 * Follows each run that fails, where pi-durable ends it: no hook sees a run
 * that fails before its request (`no_model`, a refused compaction) or after
 * a kill (`interrupted`, `restart_limit`). A kill before the follow loses
 * its notice and its wait, and a harness opened again is followed from its
 * first response.
 */
export function failedTurns(
    peers: Peers,
    withHarness: WithHarness,
    tell: TellFailed,
    ended: TurnEnded,
): FailedTurns {
    // Each request's task's last answer, until its run ends: the
    // error item an error leaves already shows its words.
    const answered = new Map<TaskId, "held" | "unsent" | "error">();
    const extension = defineExtension({
        name: "hamlet.failed",
        hooks: [
            hook(GenerationTask, {
                afterResponse: async (message, api) => {
                    // The wrapper opens a harness again after a failed
                    // commit, which the wake never saw.
                    await withHarness(async (harness) => follow(harness)).catch(
                        (error: unknown) => {
                            if (!(error instanceof Closing)) throw error;
                        },
                    );
                    const unsent = unsentOf(message);
                    if (unsent !== undefined) {
                        answered.set(
                            api.taskId,
                            unsent.held ? "held" : "unsent",
                        );
                        if (!unsent.held) {
                            await refused(
                                api.conversationId,
                                api.taskId,
                                unsent.words,
                            );
                        }
                    } else if (
                        message.stopReason === "error" ||
                        message.stopReason === "aborted"
                    ) {
                        answered.set(api.taskId, "error");
                    } else {
                        answered.delete(api.taskId);
                    }
                },
            }),
        ],
    });
    const following = new WeakSet<Harness>();
    return { extension, follow };

    function follow(harness: Harness): void {
        if (following.has(harness)) return;
        following.add(harness);
        harness.observeCommits({
            advance: ({ changes }) => {
                for (const run of failedIn(changes)) {
                    // An observer may not call the harness.
                    queueMicrotask(() => {
                        void followed(run);
                        void ended(run.conversation);
                    });
                }
            },
            end: () => {},
        });
    }

    /**
     * pi-durable never retries the gate's ending, so the turn ends here.
     * The notice's request id is the request's task, which a rerun after a
     * kill keeps: a rerun that finds the notice tells nothing again.
     */
    async function refused(
        conversation: ConversationId,
        taskId: TaskId,
        words: string,
    ): Promise<void> {
        const input = await peers.turns.of(conversation);
        const noticed = await noticeOnce(
            conversation,
            `refused:${taskId}`,
            words,
        );
        if (!noticed || input.kind === "reflection") return;
        // Not awaited: the run ends without the realm's answer.
        void tell(
            {
                conversation,
                task: input.task.task,
                words,
                cause: "budget",
                question: input.question,
            },
            undefined,
        );
    }

    async function followed({
        conversation,
        failure,
        input,
    }: FailedRun): Promise<void> {
        const last = answered.get(failure.taskId);
        answered.delete(failure.taskId);
        if (last === "held") {
            await withHarness((harness, context) =>
                harness.commit((tx) => placeWrites(tx, conversation), context),
            ).catch((error: unknown) => {
                if (!(error instanceof Closing)) {
                    peers.report(
                        "a held turn's notices were not placed",
                        error,
                    );
                }
            });
            return;
        }
        if (last === "unsent") return;
        // A run that failed before its request keeps its words on the failure.
        const said =
            typeof input.detail === "string"
                ? input.detail
                : (failure.detail ?? input.reason);
        // The gate's words in an error item are its failure to ask the
        // realm, which refused nothing.
        const realm = last === "error" ? undefined : realmWordsIn(said);
        const words = realm ?? said;
        try {
            const at = input.entry;
            const entry =
                at === undefined
                    ? undefined
                    : await withHarness((harness, context) =>
                          harness.commit((tx) => tx.entry(at), context),
                      );
            const kept = entry?.data as Kept | null | undefined;
            if (STOPPED.has(failure.reason)) {
                // A stopped turn tells only its asker.
                if (kept?.question !== undefined) {
                    await couldNotAnswer(
                        peers,
                        kept.question,
                        "its turn was stopped",
                        undefined,
                    );
                }
                return;
            }
            if (
                last !== "error" &&
                !(await noticeOnce(
                    conversation,
                    `failed:${failure.taskId}`,
                    words,
                ))
            ) {
                return;
            }
            // A reflection's failure opens no wait.
            if (entry?.kind === "hamlet.reflection") return;
            await tell(
                {
                    conversation,
                    task: kept?.task,
                    words,
                    cause: realm === undefined ? "model" : "budget",
                    question: kept?.question,
                },
                undefined,
            );
        } catch (error) {
            if (!(error instanceof Closing)) {
                peers.report("a failed turn was not followed", error);
            }
        }
    }

    /** Whether this wrote the notice: a repeat finds it written. */
    function noticeOnce(
        conversation: ConversationId,
        requestId: string,
        words: string,
    ): Promise<boolean> {
        return withHarness(async (harness, context) => {
            const before = await harness.commit(
                (tx) => tx.submissionByRequest(conversation, requestId),
                context,
            );
            if (before !== undefined) return false;
            await (await harness.conversation(conversation, context))?.submit(
                notice(requestId, words),
                context,
            );
            return true;
        });
    }
}

/**
 * Places the writes queued behind a run that ended on a held request, as
 * the end of an answered run would: pi-durable keeps them queued behind a
 * failed run until another submission comes, and a notice of the chief's
 * ruling may be the last one. A reset and what is queued after it wait for
 * pi-durable's own boundary, which places them in their order.
 */
async function placeWrites(
    tx: Tx,
    conversation: ConversationId,
): Promise<void> {
    if ((await tx.doc(LiveDoc, conversation)).run !== undefined) return;
    const { items } = await tx.doc(InboxDoc, conversation);
    for (let at = 0; at < items.length; ) {
        const item = items[at];
        if (item?.mode !== "write") {
            at += 1;
            continue;
        }
        if (item.entry.head !== undefined) return;
        const entry = await tx.appendEntry(
            conversation,
            item.entry as unknown as EntryDraft,
        );
        tx.placeSubmission(item.id, entry.id);
        items.splice(at, 1);
    }
}

function failedIn(changes: readonly CommitChange[]): FailedRun[] {
    const settled = new Map<SubmissionId, Unanswered>();
    for (const change of changes) {
        if (
            change.type === "submission" &&
            change.value.status === "unanswered"
        ) {
            settled.set(change.value.id, change.value);
        }
    }
    return changes.flatMap((change) => {
        if (
            change.type !== "document" ||
            change.kind !== "change" ||
            change.record.kind !== LiveDoc.definition.kind ||
            change.conversationId === undefined
        ) {
            return [];
        }
        // `runFailure` stays until the next run starts: the run failed in
        // this commit only when its input was settled in it too.
        const failure = (change.value as LiveState).runFailure;
        const input =
            failure?.inputs[0] === undefined
                ? undefined
                : settled.get(failure.inputs[0]);
        return failure === undefined || input === undefined
            ? []
            : [{ conversation: change.conversationId, failure, input }];
    });
}
