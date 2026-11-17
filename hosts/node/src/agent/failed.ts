/**
 * A turn that ended with no answer. Its words go to its conversation as a
 * notice, and a turn in the chief's current session tells the realm, which
 * opens a wait for the chief. The notice is committed first, so a
 * `turn_failed` lost loses only the wait.
 */
import type { Context } from "@earendil-works/chord/context";
import {
    type ConversationId,
    defineExtension,
    defineTask,
    type EntryId,
    type EntryRecord,
    type Extension,
    GenerationTask,
    hook,
    type TaskId,
    type TaskRuntime,
} from "@earendil-works/pi-durable";
import { notice } from "./commands.ts";
import type { RealmTask, TurnFailed } from "./messages.ts";
import { unsentOf } from "./spend.ts";
import {
    Closing,
    cutOff,
    type Peers,
    refOf,
    type TurnInput,
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
    const task = (input?.data as { task?: RealmTask } | null)?.task;
    await tell(
        { conversation: runtime.conversationId, task, words, cause: "model" },
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
                await failTurn(runtime, input, EMPTY, tell, context);
            },
        },
        abort: (_task, runtime, context) =>
            runtime.commit(
                () => ({ status: "terminal", outcome: { status: "aborted" } }),
                context,
            ),
    });
}

export function failedTurns(
    peers: Peers,
    withHarness: WithHarness,
    tell: TellFailed,
): Extension {
    // A retried request answers again on the same task.
    const followed = new Set<TaskId>();
    return defineExtension({
        name: "hamlet.failed",
        hooks: [
            hook(GenerationTask, {
                afterResponse: async (message, api, context) => {
                    const unsent = unsentOf(message);
                    if (unsent?.held === true) return;
                    if (unsent === undefined && message.stopReason !== "error")
                        return;
                    const input = await peers.turns.of(api.conversationId);
                    if (unsent !== undefined) {
                        await refused(
                            api.conversationId,
                            api.taskId,
                            input,
                            unsent.words,
                            context.abortSignal,
                        );
                        return;
                    }
                    // Today a reflection's failure opens no wait
                    // (`crates/hamlet_server/src/talk/follow.rs:168-173`).
                    if (input.kind === "reflection" || followed.has(api.taskId))
                        return;
                    followed.add(api.taskId);
                    void ended(api.conversationId, api.taskId, input);
                },
            }),
        ],
    });

    /**
     * pi-durable never retries the gate's ending, so the turn ends here.
     * The notice's request id is the request's task, which a rerun after a
     * kill keeps: a rerun that finds the notice tells nothing again.
     */
    async function refused(
        conversation: ConversationId,
        taskId: TaskId,
        input: TurnInput,
        words: string,
        signal: AbortSignal | undefined,
    ): Promise<void> {
        const requestId = `refused:${taskId}`;
        const noticed = await withHarness(async (harness, context) => {
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
        if (!noticed || input.kind === "reflection") return;
        await tell(
            { conversation, task: input.task.task, words, cause: "budget" },
            signal,
        );
    }

    /**
     * Only the request's task knows whether pi-durable tries again, so the
     * realm is told once it has failed. A kill before then loses the wait.
     */
    async function ended(
        conversation: ConversationId,
        taskId: TaskId,
        input: TurnInput,
    ): Promise<void> {
        try {
            const settled = await withHarness((harness, context) =>
                harness.waitForTask(taskId, context),
            );
            const { outcome } = settled.state;
            if (
                outcome.status !== "failed" ||
                (outcome.error.detail as { reason?: string } | undefined)
                    ?.reason !== "model_error"
            ) {
                return;
            }
            await tell(
                {
                    conversation,
                    task: input.task.task,
                    words: outcome.error.message,
                    cause: "model",
                },
                undefined,
            );
        } catch (error) {
            if (!(error instanceof Closing)) {
                peers.report("a failed turn was not followed", error);
            }
        }
    }
}
