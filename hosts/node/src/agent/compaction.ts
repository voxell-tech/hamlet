/**
 * Compaction's settings by each listed model's window, and the short
 * cut-off rule (C28): an empty answer cut off short, with history, on its
 * first try, compacts once and runs again. Any other empty answer fails
 * its turn (C29).
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
} from "@earendil-works/pi-durable";
import { emptyAnswer, failTurn, type TellFailed } from "./failed.ts";
import type { LetterId, RealmTask } from "./messages.ts";
import { realmWordsIn } from "./spend.ts";
import { draftOf } from "./turn.ts";

/** Today's, in `harnesses/pi/src/models.ts` and `runtime.ts`. */
const ANSWER_SHARE = 0.2;
const BACKGROUND_SHARE = 0.1;
const DEFAULT_WINDOW = 32_768;

/** Compaction sized by each model's window, and by today's default window for any other. */
export function compactionFor(
    models: readonly Model<Api>[],
): CompactionSettings {
    return {
        keepRecentTokens: 20_000,
        ...sized(DEFAULT_WINDOW),
        modelOverrides: Object.fromEntries(
            models.map((model) => [
                `${model.provider}/${model.id}`,
                sized(model.contextWindow),
            ]),
        ),
    };
}

function sized(window: number) {
    return {
        reserveTokens: Math.round(window * ANSWER_SHARE),
        backgroundTokens: Math.round(window * BACKGROUND_SHARE),
    };
}

/** Today's rule and words, in `crates/hamlet_server/src/talk/verdicts.rs`. */
const ROOMY = 1024;

const ASKED_AGAIN = "hamlet.cut-off";

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

/** An answer cut off with nothing in it, which the rule asks again once. */
export function isShortCutOff(answer: AssistantMessage): boolean {
    return (
        !hasWordsOrCall(answer) &&
        answer.stopReason === "length" &&
        answer.usage.output < ROOMY
    );
}

/** Today's test of an answer that stands (`verdicts.rs:55-60`): words or a tool call. */
export function hasWordsOrCall(answer: AssistantMessage): boolean {
    return answer.content.some(
        (block) =>
            block.type === "toolCall" ||
            (block.type === "text" && block.text.trim() !== ""),
    );
}

function cutOffTask(tell: TellFailed) {
    return defineTask<CutOffInput, CutOffState, null>({
        name: "hamlet.cut-off",
        version: 1,
        initial: () => ({ phase: "rule" }),
        phases: {
            rule: async (task, runtime, context) => {
                const input = await runtime.entry(task.input.input, context);
                if (input?.kind === ASKED_AGAIN) {
                    return failTurn(
                        runtime,
                        input,
                        NO_ROOM,
                        "model",
                        tell,
                        context,
                    );
                }
                const view = await runtime.context(
                    runtime.conversationId,
                    context,
                    { at: task.input.answer },
                );
                const spoken = view.messages.filter(
                    (message) => message.role !== "system",
                ).length;
                if (spoken <= 2) {
                    return failTurn(
                        runtime,
                        input,
                        TOO_LONG,
                        "model",
                        tell,
                        context,
                    );
                }
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
                const input = await runtime.entry(task.input.input, context);
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
                    const realm = realmWordsIn(error);
                    if (realm !== undefined) {
                        return failTurn(
                            runtime,
                            input,
                            realm,
                            "budget",
                            tell,
                            context,
                        );
                    }
                    return failTurn(
                        runtime,
                        input,
                        error.startsWith(SUMMARY_FAILED)
                            ? `provider error: ${error.slice(SUMMARY_FAILED.length)}`
                            : NO_SUMMARY,
                        "model",
                        tell,
                        context,
                    );
                }
                const { task: charged, question } =
                    (input?.data as {
                        task?: RealmTask;
                        question?: LetterId;
                    } | null) ?? {};
                const asked = draftOf({
                    id: `cut-off:${runtime.taskId}`,
                    kind: "cut-off",
                    content: CARRY_ON,
                    ...(charged === undefined ? {} : { task: charged }),
                    ...(question === undefined ? {} : { question }),
                });
                const conversation = await runtime.conversation(
                    runtime.conversationId,
                    context,
                );
                await runtime.commit(async (tx) => {
                    await conversation?.submit(tx, asked);
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
}

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

export function cutOffRule(tell: TellFailed): Extension {
    const CutOff = cutOffTask(tell);
    const EmptyAnswer = emptyAnswer(tell);
    return defineExtension({
        name: "hamlet.cut-off",
        tasks: [CutOff, EmptyAnswer],
        hooks: [
            hook(GenerationTask, {
                afterRun: async (run, api, context) => {
                    const answer = (await api.entry(run.answer, context))
                        ?.model?.[0];
                    if (
                        answer?.role !== "assistant" ||
                        hasWordsOrCall(answer)
                    ) {
                        return undefined;
                    }
                    const input = await inputOf(api, run, context);
                    // Today's rule leaves a reflection's empty answer to C29.
                    const task =
                        isShortCutOff(answer) &&
                        input !== undefined &&
                        input.kind !== "hamlet.reflection"
                            ? runEndTask(CutOff, {
                                  input: input.id,
                                  answer: run.answer,
                              })
                            : runEndTask(EmptyAnswer, {
                                  input: input?.id ?? null,
                              });
                    return { tasks: [task] };
                },
            }),
        ],
    });
}
