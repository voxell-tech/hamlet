import type { Context } from "@earendil-works/chord";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import {
    type ConversationId,
    defineDoc,
    defineExtension,
    GenerationTask,
    hook,
    InboxDoc,
    LiveDoc,
    SessionFailed,
    type SubmissionId,
    type Tx,
} from "@earendil-works/pi-durable";
import { closeGate } from "../gate.ts";
import type { Home, Part } from "../home.ts";
import { raiseFault } from "../oversight/waits.ts";
import { addressOf, agentKey, RecordDoc } from "../record.ts";
import { reconcile } from "../spend/ledger.ts";
import {
    answerFailure,
    checkAnswer,
    recoverAnswers,
    reportAnswerFailures,
} from "./answers.ts";
import {
    ChiefEntry,
    ReflectionEntry,
    RetryEntry,
    type With,
    WithDoc,
} from "./conversations.ts";
import { InstructionsDoc } from "./instructions.ts";
import { NoticeEntry } from "./notice.ts";
import { agentConversations } from "./owned.ts";
import {
    enterReflection,
    ReflectDoc,
    ReflectionDoc,
    reflectEvery,
    requestReflection,
} from "./reflection.ts";
import { turnOf } from "./turn.ts";

const FollowDoc = defineDoc<{
    pending: SubmissionId[];
    seen: Record<string, boolean>;
    last: "done" | "failed" | "stopped" | null;
    input: SubmissionId | null;
}>({
    kind: "hamlet.follow",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "initial",
    initial: () => ({ pending: [], seen: {}, last: null, input: null }),
});

async function rememberInput(
    tx: Tx,
    conversation: ConversationId,
    input: SubmissionId,
): Promise<void> {
    const follow = await tx.doc(FollowDoc, conversation);
    if (!follow.seen[String(input)] && !follow.pending.includes(input))
        follow.pending.push(input);
}

/** Serialized follow-up work for agent conversations, carrying queued inputs across a failure (C23). */
export const Talk: Part = (home) => {
    const admitted = new Map<ConversationId, Set<SubmissionId>>();
    let line = Promise.resolve();
    let unsubscribe: (() => void) | undefined;
    let closing = false;
    const remember = async (
        conversation: ConversationId,
        input: SubmissionId,
        context: Context,
    ) => {
        await home.harness.commit(
            (tx) => rememberInput(tx, conversation, input),
            context,
        );
    };
    const drain = async (context: Context) => {
        const conversations = [...admitted];
        admitted.clear();
        for (const [conversation, ids] of conversations) {
            for (const id of ids) await remember(conversation, id, context);
            await follow(home, conversation, context);
        }
    };
    const flush = (context: Context) => {
        const pending = line.then(() => drain(context));
        line = pending.catch((error: unknown) => {
            if (!(error instanceof SessionFailed)) throw error;
        });
        void line.catch(() => {});
        return pending;
    };

    return {
        follow: { settled: flush },
        extension: defineExtension({
            name: "hamlet.talk",
            hooks: [
                hook(GenerationTask, {
                    onYield: checkAnswer(home),
                    beforeRequest: async (_request, api, context) => {
                        const turn = await turnOf(home.harness, api, context);
                        if (turn !== undefined) {
                            await remember(
                                api.conversationId,
                                turn.input,
                                context,
                            );
                            await home.harness.commit(
                                (tx) =>
                                    enterReflection(
                                        tx,
                                        api.conversationId,
                                        turn,
                                    ),
                                context,
                            );
                        }
                        return undefined;
                    },
                }),
            ],
        }),
        async start(context) {
            unsubscribe = home.harness.subscribeCommits(({ changes }) => {
                for (const change of changes) {
                    if (
                        change.type === "task" &&
                        change.value.kind === "pi.compaction" &&
                        change.value.state.status === "terminal"
                    ) {
                        if (!admitted.has(change.value.conversationId))
                            admitted.set(
                                change.value.conversationId,
                                new Set(),
                            );
                    }
                    if (
                        change.type !== "submission" ||
                        change.value.type !== "input"
                    )
                        continue;
                    const submission = change.value;
                    const ids =
                        admitted.get(submission.conversationId) ??
                        new Set<SubmissionId>();
                    ids.add(submission.id);
                    admitted.set(submission.conversationId, ids);
                }
                if (admitted.size > 0 && !closing)
                    void flush(BACKGROUND_CONTEXT).catch(() => {});
            });
            for (const { id } of await home.harness.commit(async (tx) => {
                const conversations = await agentConversations(tx);
                for (const { id, place } of conversations) {
                    const key = agentKey(place);
                    const prefix = `chief:${id}:`;
                    for (const instruction of (
                        await tx.doc(InstructionsDoc, key, key)
                    ).given) {
                        if (
                            instruction.done !== null ||
                            !instruction.task.startsWith(prefix)
                        )
                            continue;
                        const submission = await tx.submissionByRequest(
                            id,
                            `chief:${instruction.task.slice(prefix.length)}`,
                        );
                        if (submission === undefined)
                            instruction.done = home.now();
                        else await rememberInput(tx, id, submission.id);
                    }
                }
                return conversations;
            }, context))
                if (!admitted.has(id)) admitted.set(id, new Set());
            await flush(context);
        },
        async stop(context) {
            closing = true;
            closeGate(home);
            unsubscribe?.();

            try {
                await line;
                await drain(context);
                if ((await reflectEvery(home, context)) !== 0) {
                    const jobs = await home.harness.commit(
                        chiefConversations,
                        context,
                    );
                    const waits: Promise<unknown>[] = [];
                    for (const { id } of jobs) {
                        const submission = await requestReflection(
                            home,
                            id,
                            context,
                            "owed",
                        );
                        if (submission === undefined) continue;
                        // A running chief turn is suspended by close, not waited on for reflection.
                        const canWait = await home.harness.commit(
                            async (tx) => {
                                const input = (await tx.doc(LiveDoc, id)).run
                                    ?.inputs[0];
                                if (input === undefined) return true;
                                const entries = await tx.scanEntries(
                                    { conversationId: id },
                                    100,
                                    undefined,
                                );
                                const entry = entries.items.find(
                                    (each) =>
                                        ChiefEntry.is(each) ||
                                        RetryEntry.is(each) ||
                                        ReflectionEntry.is(each),
                                );
                                return ReflectionEntry.is(entry);
                            },
                            context,
                        );
                        if (!canWait) continue;
                        waits.push(submission.wait(context));
                    }
                    let timer: ReturnType<typeof setTimeout> | undefined;
                    try {
                        await Promise.race([
                            Promise.all(waits),
                            new Promise<void>((resolve) => {
                                timer = setTimeout(resolve, 120_000);
                            }),
                        ]);
                    } finally {
                        clearTimeout(timer);
                    }
                    await drain(context);
                }
                await reconcile(home, context);
            } catch (error) {
                if (!(error instanceof SessionFailed)) throw error;
            }
        },
    };
};

async function chiefConversations(
    tx: Tx,
): Promise<{ id: ConversationId; place: With & { chief: string } }[]> {
    return (await agentConversations(tx)).flatMap(({ id, place }) =>
        place.chief === null
            ? []
            : [{ id, place: { ...place, chief: place.chief } }],
    );
}

async function follow(
    home: Home,
    id: ConversationId,
    context: Context,
): Promise<void> {
    const place = await home.harness.snapshot(WithDoc, id, context);
    if (place === undefined) return;
    for (const input of (await home.harness.snapshot(FollowDoc, id, context))
        ?.pending ?? []) {
        const submission = await home.harness.submission(input, context);
        const status = await submission?.status(context);
        if (
            status === undefined ||
            status.status === "queued" ||
            status.status === "placed"
        )
            continue;
        const failure = await answerFailure(home, id, input, context);
        await home.harness.commit(async (tx) => {
            const follow = await tx.doc(FollowDoc, id);
            if (follow.seen[String(input)]) return;
            follow.seen[String(input)] = true;
            follow.pending = follow.pending.filter((each) => each !== input);
            follow.input = input;
            follow.last =
                status.status === "done"
                    ? "done"
                    : failure !== undefined
                      ? "failed"
                      : status.reason === "model_error" ||
                          status.reason === "no_model"
                        ? "failed"
                        : "stopped";
            const entry =
                status.entry === undefined
                    ? undefined
                    : await tx.entry(status.entry);
            const chiefTask = ChiefEntry.is(entry)
                ? entry.data.task
                : status.requestId?.startsWith("chief:")
                  ? `chief:${id}:${status.requestId.slice("chief:".length)}`
                  : undefined;
            if (chiefTask !== undefined) {
                const key = agentKey(place);
                const instruction = (
                    await tx.doc(InstructionsDoc, key, key)
                ).given.find((i) => i.task === chiefTask);
                if (instruction) instruction.done = home.now();
            }
            if (entry !== undefined && ReflectionEntry.is(entry)) {
                const job = await tx.doc(ReflectDoc, id);
                if (job.entered !== input) {
                    job.entered = input;
                    (await tx.doc(ReflectionDoc, id)).unreflected = 0;
                }
                job.pending = job.pending.filter(
                    (each) => each.task !== entry.data.task,
                );
            } else if (entry === undefined) {
                const job = await tx.doc(ReflectDoc, id);
                for (const reserved of [...job.pending]) {
                    const admitted = await tx.submissionByRequest(
                        id,
                        reserved.requestId,
                    );
                    if (admitted?.id === input)
                        job.pending = job.pending.filter(
                            (each) => each.requestId !== reserved.requestId,
                        );
                }
            } else if (
                status.status === "done" &&
                (ChiefEntry.is(entry) || RetryEntry.is(entry))
            ) {
                (await tx.doc(ReflectionDoc, id)).unreflected++;
            }
            if (
                follow.last === "failed" &&
                failure === undefined &&
                !(entry !== undefined && ReflectionEntry.is(entry))
            ) {
                const record = await tx.doc(
                    RecordDoc,
                    place.hamlet,
                    place.hamlet,
                );
                await raiseFault(
                    tx,
                    {
                        by: addressOf(record, place.number),
                        agent: { hamlet: place.hamlet, number: place.number },
                        chat: { ...place },
                        about:
                            typeof status.detail === "string"
                                ? status.detail
                                : status.status === "unanswered"
                                  ? status.reason
                                  : "The model failed.",
                    },
                    home.now(),
                );
            }
        }, context);
    }
    const recovering = await recoverAnswers(home, id, context);
    await reportAnswerFailures(home, id, context);
    if (recovering) return;
    const state = await home.harness.snapshot(FollowDoc, id, context);
    const job = await home.harness.snapshot(ReflectDoc, id, context);
    if (state?.last !== "stopped" || (job?.pending.length ?? 0) > 0)
        await requestReflection(home, id, context);
    const live = await home.harness.snapshot(LiveDoc, id, context);
    const inbox = await home.harness.snapshot(InboxDoc, id, context);
    if (
        live?.run === undefined &&
        (inbox?.items.length ?? 0) > 0 &&
        state?.input != null
    ) {
        const conversation = await home.harness.conversation(id, context);
        await conversation?.submit(
            {
                type: "write",
                requestId: `queue:${state?.input}`,
                entry: {
                    kind: NoticeEntry.kind,
                    data: {
                        text: "Continuing the queued messages.",
                        at: home.now(),
                    },
                    edits: [],
                },
            },
            context,
        );
    }
}
