/**
 * A turn that ended with no answer. Its words go to its conversation as a
 * notice, and a turn in the chief's current session tells the realm, which
 * opens a wait for the chief. The notice is committed first, so a
 * `turn_failed` lost loses only the wait.
 */
import type { Context } from "@earendil-works/chord/context";
import {
    type CommitChange,
    type ConversationId,
    defineExtension,
    defineTask,
    type EntryId,
    type EntryRecord,
    type Extension,
    GenerationTask,
    type Harness,
    hook,
    LiveDoc,
    type LiveState,
    type RunFailure,
    type RunFailureReason,
    type SubmissionId,
    type SubmissionRecord,
    type TaskId,
    type TaskRuntime,
} from "@earendil-works/pi-durable";
import { notice } from "./commands.ts";
import type { RealmTask, TurnFailed } from "./messages.ts";
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
}

/** Tells the realm of a failure in the chief's current session; never throws. */
export type TellFailed = (
    failure: Failure,
    signal: AbortSignal | undefined,
) => Promise<void>;

export function tellingFailed(
    peers: Peers,
    standing: RealmTask,
    chief: () => Promise<ConversationId | undefined>,
): TellFailed {
    return async ({ conversation, task, words, cause }, signal) => {
        try {
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
    const task = (input?.data as Kept | null)?.task;
    await tell(
        { conversation: runtime.conversationId, task, words, cause },
        context.abortSignal,
    );
}

/** Today's (`crates/hamlet_server/src/talk/verdicts.rs:19-20`). */
const EMPTY = "the model answered with nothing. Send the message again";

type EmptyInput = { input: EntryId | null };

/**
 * C29, for an empty answer C28 does not ask again. pi-durable settled the
 * run's input answered, so it fails here, as today's
 * `rule` in `crates/hamlet_server/src/talk/verdicts.rs:44-138` fails it.
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

export interface FailedTurns {
    readonly extension: Extension;
    /** Follows every run of `harness` that fails from now on. */
    follow(harness: Harness): void;
}

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
): FailedTurns {
    const answered = new Map<TaskId, "unsent" | "error">();
    const extension = defineExtension({
        name: "hamlet.failed",
        hooks: [
            hook(GenerationTask, {
                afterResponse: async (message, api) => {
                    await withHarness(async (harness) => follow(harness)).catch(
                        (error: unknown) => {
                            if (!(error instanceof Closing)) throw error;
                        },
                    );
                    const unsent = unsentOf(message);
                    if (unsent !== undefined) {
                        answered.set(api.taskId, "unsent");
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
                    queueMicrotask(() => void followed(run));
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
        void tell(
            { conversation, task: input.task.task, words, cause: "budget" },
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
        if (last === "unsent" || STOPPED.has(failure.reason)) return;
        // As today's `fault` words it (`talk/follow.rs:436-446`); a run
        // that failed before its request keeps its words on the failure.
        const said =
            typeof input.detail === "string"
                ? input.detail
                : (failure.detail ?? input.reason);
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
            // Today a reflection's failure opens no wait
            // (`crates/hamlet_server/src/talk/follow.rs:168-173`).
            if (entry?.kind === "hamlet.reflection") return;
            await tell(
                {
                    conversation,
                    task: (entry?.data as Kept | null)?.task,
                    words,
                    cause: realm === undefined ? "model" : "budget",
                },
                undefined,
            );
        } catch (error) {
            if (!(error instanceof Closing)) {
                peers.report("a failed turn was not followed", error);
            }
        }
    }

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
