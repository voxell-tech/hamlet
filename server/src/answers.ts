import { randomUUID } from "node:crypto";
import type { Context, JsonRepresentation } from "@earendil-works/chord";
import type { AssistantMessage } from "@earendil-works/pi-ai";
import {
    type CompactionResult,
    CompactionTask,
    type ConversationId,
    defineDoc,
    defineDocFamily,
    type GenerationHooks,
    LiveDoc,
    type SubmissionId,
    type TaskId,
    type Tx,
} from "@earendil-works/pi-durable";
import { ReflectionEntry, retryInput, WithDoc } from "./chats.ts";
import type { Home } from "./home.ts";
import { untilAborted } from "./home.ts";
import { notify } from "./notice.ts";
import { raiseFault } from "./oversight.ts";
import { addressOf, RecordDoc } from "./record.ts";
import { safeWords } from "./scrub.ts";
import { turnOf } from "./turn.ts";

export const EMPTY = "the model answered with nothing. Send the message again";
export const NO_ROOM =
    "the model has no room left to answer, even with the conversation summarised. Start a new session, or choose a model that holds more";
export const NO_SUMMARY =
    "the conversation is longer than the model can hold, and the model could not summarise it. Start a new session, or choose a model that holds more";
export const SUMMARY_NOTICE =
    "The conversation grew longer than the model can hold, so the earlier part of it was summarised.";

type Mark = {
    reported: boolean;
    words: string;
    task: string | null;
    retry: string;
    at: number;
    status: "failed" | "waiting" | "summarizing" | "submitted" | "cancelled";
    compaction: TaskId<CompactionResult> | null;
};
/** The real cause of an aborted empty answer and its resumable recovery (C28, C29). */
export const AnswerDoc = defineDoc<{ marks: Record<string, Mark> }>({
    kind: "hamlet.answers",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "initial",
    initial: () => ({ marks: {} }),
});
const CutsDoc = defineDocFamily<{ count: number }, string>({
    kind: "hamlet.cutoffs",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ count: 0 }),
});

/** Checks only final answers; reasoning alone is not an answer. */
export function checkAnswer(
    home: Home,
): NonNullable<GenerationHooks["onYield"]> {
    return async (message, api, context) => {
        if (
            message.content.some(
                (part) =>
                    part.type === "toolCall" ||
                    (part.type === "text" && part.text.trim() !== ""),
            )
        )
            return undefined;
        const turn = await turnOf(home.harness, api, context);
        const cutoff =
            message.stopReason === "length" &&
            message.usage.output < 1024 &&
            turn?.kind !== ReflectionEntry.kind &&
            turn?.task != null;
        await home.harness.commit(async (tx) => {
            const cut = cutoff
                ? await tx.doc(
                      CutsDoc,
                      turn.task as string,
                      turn.task as string,
                  )
                : undefined;
            const again = cut !== undefined && ++cut.count === 1;
            const words = cutoff ? NO_ROOM : EMPTY;
            (await tx.doc(AnswerDoc, api.conversationId)).marks[
                String(turn?.input)
            ] = {
                reported: false,
                words,
                task: turn?.task ?? null,
                retry: `retry:${randomUUID()}`,
                at: home.now(),
                status: again ? "waiting" : "failed",
                compaction: null,
            };
            const live = await tx.doc(LiveDoc, api.conversationId);
            // Keep the complete response before abort converts it, including its metered usage.
            if (live.generation !== undefined)
                live.generation.message = JSON.parse(
                    JSON.stringify({
                        ...message,
                        errorMessage: again
                            ? "The answer was cut off for room. Summarising before trying once more."
                            : words,
                    }),
                ) as JsonRepresentation<AssistantMessage>;
            if (!again) await notify(tx, api.conversationId, words, home.now());
        }, context);
        void home.harness.abortTask(api.taskId, context).catch(() => {});
        return untilAborted(context);
    };
}

export async function answerFailure(
    home: Home,
    id: ConversationId,
    input: SubmissionId,
    context: Context,
): Promise<string | undefined> {
    const mark = (await home.harness.snapshot(AnswerDoc, id, context))?.marks[
        String(input)
    ];
    return mark?.status === "failed" ? mark.words : undefined;
}

/** Cancels recovery with the chief's stop, before its task is aborted. */
export async function cancelAnswers(tx: Tx, id: ConversationId): Promise<void> {
    for (const mark of Object.values((await tx.doc(AnswerDoc, id)).marks))
        if (mark.status === "waiting" || mark.status === "summarizing")
            mark.status = "cancelled";
}

/** Advances the durable cut-off jobs without waiting on model work on the follower's line. */
export async function recoverAnswers(
    home: Home,
    id: ConversationId,
    context: Context,
): Promise<boolean> {
    const state = await home.harness.snapshot(AnswerDoc, id, context);
    for (const [input, mark] of Object.entries(state?.marks ?? {})) {
        if (mark.status === "waiting") {
            const conversation = await home.harness.conversation(id, context);
            const active = await conversation?.context(context);
            if (
                (active?.messages.filter((message) => message.role !== "system")
                    .length ?? 0) <= 1
            ) {
                await home.harness.commit(async (tx) => {
                    const current = (await tx.doc(AnswerDoc, id)).marks[input];
                    if (current?.status !== "waiting") return;
                    current.status = "failed";
                    current.words =
                        "the message is longer than the model can hold";
                    await notify(tx, id, current.words, home.now());
                }, context);
                continue;
            }
            await home.harness.commit(async (tx) => {
                const current = (await tx.doc(AnswerDoc, id)).marks[input];
                if (current?.status !== "waiting") return;
                // Public tasks and LiveDoc make admission atomic with the recovery journal.
                const task = await tx.createTask(
                    CompactionTask,
                    { reason: "manual" },
                    { ownership: { kind: "conversation" }, conversationId: id },
                );
                const live = await tx.doc(LiveDoc, id);
                live.compactions ??= [];
                live.compactions.push({
                    taskId: task,
                    reason: "manual",
                    blocking: false,
                    attempt: 1,
                });
                current.compaction = task;
                current.status = "summarizing";
            }, context);
            return true;
        }
        if (mark.status !== "summarizing" || mark.compaction === null) continue;
        const task = await home.harness.getTask(mark.compaction, context);
        if (task?.state.status !== "terminal") return true;
        const outcome = task.state.outcome;
        if (
            outcome.status !== "completed" ||
            (outcome.result.entryId === undefined &&
                outcome.result.submissionId === undefined)
        ) {
            await home.harness.commit(async (tx) => {
                const current = (await tx.doc(AnswerDoc, id)).marks[input];
                if (current === undefined) return;
                current.status =
                    outcome.status === "aborted" ? "cancelled" : "failed";
                current.words = safeWords(
                    home,
                    (outcome.status === "failed" ||
                        outcome.status === "faulted") &&
                        !/^Summarization (produced|hit|attempted)/.test(
                            outcome.error.message,
                        )
                        ? `provider error: ${outcome.error.message.replace(/^Summarization failed: /, "")}`
                        : NO_SUMMARY,
                );
                if (current.status === "failed")
                    await notify(tx, id, current.words, home.now());
            }, context);
            continue;
        }
        if (outcome.result.submissionId !== undefined) {
            const summary = await home.harness.submission(
                outcome.result.submissionId,
                context,
            );
            if ((await summary?.status(context))?.status !== "done")
                return true;
        }
        const conversation = await home.harness.conversation(id, context);
        if (conversation !== undefined && mark.task !== null)
            await conversation.submit(
                { ...retryInput(mark.task, mark.at), requestId: mark.retry },
                context,
            );
        await home.harness.commit(async (tx) => {
            const current = (await tx.doc(AnswerDoc, id)).marks[input];
            if (current?.status === "summarizing") current.status = "submitted";
        }, context);
    }
    return false;
}

/** Reports each answer failure once, including a summary that failed after the input settled. */
export async function reportAnswerFailures(
    home: Home,
    id: ConversationId,
    context: Context,
): Promise<void> {
    await home.harness.commit(async (tx) => {
        const state = await tx.doc(AnswerDoc, id);
        const place = await tx.doc(WithDoc, id);
        if (place.chief === null) return;
        for (const mark of Object.values(state.marks)) {
            if (mark.status !== "failed" || mark.reported) continue;
            mark.reported = true;
            const record = await tx.doc(RecordDoc, place.hamlet, place.hamlet);
            await raiseFault(
                tx,
                {
                    by: addressOf(record, place.number),
                    agent: { hamlet: place.hamlet, number: place.number },
                    chat: { ...place },
                    about: mark.words,
                },
                home.now(),
            );
        }
    }, context);
}
