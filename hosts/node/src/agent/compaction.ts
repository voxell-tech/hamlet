/**
 * Compaction's settings by each listed model's window, and the short
 * cut-off rule (C28): an empty answer cut off short, with history, on its
 * first try, compacts once and runs again.
 */
import type { Context } from "@earendil-works/chord/context";
import type { Api, AssistantMessage, Model } from "@earendil-works/pi-ai";
import {
    type CompactionResult,
    type CompactionSettings,
    CompactionTask,
    defineExtension,
    defineTask,
    type EntryId,
    type EntryRecord,
    type Extension,
    GenerationTask,
    type HookApi,
    hook,
    type RunSummary,
    runEndTask,
    type TaskId,
    type TaskRuntime,
} from "@earendil-works/pi-durable";
import type { RealmTask } from "./messages.ts";
import { draftOf } from "./turn.ts";

/** Today's, in `harnesses/pi/src/models.ts` and `runtime.ts`. */
const ANSWER_SHARE = 0.2;
const BACKGROUND_SHARE = 0.1;

/** Compaction sized by each model's window. */
export function compactionFor(
    models: readonly Model<Api>[],
): CompactionSettings {
    return {
        keepRecentTokens: 20_000,
        modelOverrides: Object.fromEntries(
            models.map((model) => [
                `${model.provider}/${model.id}`,
                {
                    reserveTokens: Math.round(
                        model.contextWindow * ANSWER_SHARE,
                    ),
                    backgroundTokens: Math.round(
                        model.contextWindow * BACKGROUND_SHARE,
                    ),
                },
            ]),
        ),
    };
}

/** Today's rule and words, in `crates/hamlet_server/src/talk/verdicts.rs`. */
const ROOMY = 1024;

/**
 * The kind of the input that asks again, and so the mark that the run was
 * tried once: a second cut-off fails.
 */
const ASKED_AGAIN = "hamlet.cut-off";

const NOTICE = "hamlet.notice";

const NO_ROOM =
    "the model has no room left to answer, even with the conversation summarised. Start a new session, or choose a model that holds more";
const NO_SUMMARY =
    "the conversation is longer than the model can hold, and the model could not summarise it. Start a new session, or choose a model that holds more";
const TOO_LONG = "the message is longer than the model can hold";
const SUMMARY_FAILED = "Summarization failed: ";

/** Today's retry input, in `crates/hamlet_server/src/talk/chats.rs`. */
const CARRY_ON = "Carry on from where you stopped.";

type CutOffInput = { input: EntryId; answer: EntryId };

type CutOffState =
    | { phase: "rule" }
    | { phase: "summarised"; summary: TaskId<CompactionResult> };

function cutOffShort(answer: AssistantMessage): boolean {
    const stands = answer.content.some(
        (block) =>
            block.type === "toolCall" ||
            (block.type === "text" && block.text.trim() !== ""),
    );
    return (
        !stands && answer.stopReason === "length" && answer.usage.output < ROOMY
    );
}

/**
 * The compaction is created in the commit that waits for it, so a kill
 * never makes a second one.
 */
const CutOff = defineTask<CutOffInput, CutOffState, null>({
    name: "hamlet.cut-off",
    version: 1,
    initial: () => ({ phase: "rule" }),
    phases: {
        rule: async (task, runtime, context) => {
            const input = await runtime.entry(task.input.input, context);
            if (input?.kind === ASKED_AGAIN) {
                return notice(runtime, NO_ROOM, context);
            }
            const view = await runtime.context(
                runtime.conversationId,
                context,
                { at: task.input.answer },
            );
            // Today counted the messages before the answer: one is no history.
            const spoken = view.messages.filter(
                (message) => message.role !== "system",
            ).length;
            if (spoken <= 2) return notice(runtime, TOO_LONG, context);
            await runtime.commit(async (tx) => {
                const summary = await tx.createTask(
                    CompactionTask,
                    { reason: "manual" },
                    { ownership: { kind: "task", taskId: runtime.taskId } },
                );
                return {
                    status: "waiting",
                    checkpoint: { phase: "summarised", summary },
                    on: [summary],
                    policy: "allSettled",
                };
            }, context);
        },
        summarised: async (task, runtime, context) => {
            const [outcome] = await runtime.outcomes(
                [task.state.checkpoint.summary],
                context,
            );
            if (
                outcome?.status !== "completed" ||
                outcome.result.entryId === undefined
            ) {
                const error =
                    outcome?.status === "failed" ||
                    outcome?.status === "faulted"
                        ? outcome.error.message
                        : "";
                return notice(
                    runtime,
                    error.startsWith(SUMMARY_FAILED)
                        ? `provider error: ${error.slice(SUMMARY_FAILED.length)}`
                        : NO_SUMMARY,
                    context,
                );
            }
            const input = await runtime.entry(task.input.input, context);
            const charged = (input?.data as { task?: RealmTask } | null)?.task;
            const asked = draftOf({
                id: `cut-off:${runtime.taskId}`,
                kind: "retry",
                content: CARRY_ON,
                ...(charged === undefined ? {} : { task: charged }),
            });
            const conversation = await runtime.conversation(
                runtime.conversationId,
                context,
            );
            await runtime.commit(async (tx) => {
                await conversation?.submit(tx, {
                    ...asked,
                    entry: { kind: ASKED_AGAIN, data: asked.entry?.data ?? {} },
                });
                return {
                    status: "terminal",
                    outcome: { status: "completed", result: null },
                };
            }, context);
        },
    },
    abort: (_task, runtime, context) =>
        runtime.commit(
            () => ({ status: "terminal", outcome: { status: "aborted" } }),
            context,
        ),
});

async function notice(
    runtime: TaskRuntime<CutOffInput, CutOffState, null, object>,
    words: string,
    context: Context,
): Promise<void> {
    const conversation = await runtime.conversation(
        runtime.conversationId,
        context,
    );
    await runtime.commit(async (tx) => {
        await conversation?.submit(tx, {
            type: "write",
            entry: {
                kind: NOTICE,
                data: { text: words, at: Math.floor(runtime.now() / 1000) },
            },
        });
        return {
            status: "terminal",
            outcome: { status: "completed", result: null },
        };
    }, context);
}

/**
 * The entry of the input the run answers. A write placed with it, such as
 * a notice, can come first.
 */
async function inputOf(
    api: Pick<HookApi, "entries">,
    run: RunSummary,
    context: Context,
): Promise<EntryRecord | undefined> {
    const page = await api.entries(
        { minEntryId: run.first, maxEntryId: run.answer, order: "ascending" },
        20,
        undefined,
        context,
    );
    return page.items.find(
        (entry) =>
            entry.kind.startsWith("hamlet.") && entry.model !== undefined,
    );
}

export function cutOff(): Extension {
    return defineExtension({
        name: "hamlet.cut-off",
        tasks: [CutOff],
        hooks: [
            hook(GenerationTask, {
                afterRun: async (run, api, context) => {
                    const answer = (await api.entry(run.answer, context))
                        ?.model?.[0];
                    if (answer?.role !== "assistant" || !cutOffShort(answer)) {
                        return undefined;
                    }
                    const input = await inputOf(api, run, context);
                    // Today's rule leaves a reflection's empty answer to C29.
                    if (
                        input === undefined ||
                        input.kind === "hamlet.reflection"
                    ) {
                        return undefined;
                    }
                    return {
                        tasks: [
                            runEndTask(CutOff, {
                                input: input.id,
                                answer: run.answer,
                            }),
                        ],
                    };
                },
            }),
        ],
    });
}
