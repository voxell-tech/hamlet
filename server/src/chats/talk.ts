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
import { configureAgent } from "../models.ts";
import { resetTrust } from "../oversight/asked.ts";
import {
    forAgent,
    lapse,
    open,
    raiseFault,
    settle,
} from "../oversight/waits.ts";
import { chiefAccess } from "../realm/access.ts";
import { liveHamlets, RealmDoc } from "../realm/places.ts";
import { type AgentRef, addressOf, agentKey, RecordDoc } from "../record.ts";
import { reconcile } from "../spend/ledger.ts";
import {
    answerFailure,
    cancelAnswers,
    checkAnswer,
    recoverAnswers,
    reportAnswerFailures,
} from "./answers.ts";
import {
    AgentConversationsDoc,
    ChatDoc,
    type ChatId,
    ChiefEntry,
    chatOf,
    currentOf,
    ReflectionEntry,
    RetryEntry,
    retryInput,
    startConversation,
    type With,
    WithDoc,
} from "./conversations.ts";
import { entriesOf, LineageDoc } from "./history.ts";
import { InstructionsDoc } from "./instructions.ts";
import { NoticeEntry, notify } from "./notice.ts";
import {
    enterReflection,
    ReflectDoc,
    ReflectionDoc,
    reflectEvery,
    requestReflection,
} from "./reflection.ts";
import { newTask, turnOf } from "./turn.ts";

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
const followers = new WeakMap<Home, (context: Context) => Promise<void>>();

async function rememberInput(
    tx: Tx,
    conversation: ConversationId,
    input: SubmissionId,
): Promise<void> {
    const follow = await tx.doc(FollowDoc, conversation);
    if (!follow.seen[String(input)] && !follow.pending.includes(input))
        follow.pending.push(input);
}

/** Settles the conversations' follow-up work already kept by pi. */
export async function followHome(home: Home, context: Context): Promise<void> {
    await followers.get(home)?.(context);
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
    followers.set(home, flush);
    return {
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
            followers.delete(home);
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

/** Stops every agent of the living hamlets this chief leads. */
export async function stopChiefAgents(
    home: Home,
    context: Context,
): Promise<string> {
    const realm = await home.harness.snapshot(RealmDoc, context);
    for (const place of realm?.hamlets ?? []) {
        const access = await chiefAccess(home, place.id, "stop", context);
        if (!access.ok) continue;
        const record = await home.harness.snapshot(
            RecordDoc,
            place.id,
            context,
        );
        for (const agent of record?.agents ?? [])
            await stopAgent(
                home,
                { hamlet: place.id, number: agent.number },
                context,
            );
    }
    return "Stopping everything.";
}

/** Stops an agent's running conversations and drops their queued inputs (C25, H34). */
export async function stopAgent(
    home: Home,
    agent: AgentRef | undefined,
    context: Context,
): Promise<string> {
    const places = await home.harness.commit(async (tx) => {
        const places: { id: ConversationId; chat: ChatId; agent: AgentRef }[] =
            [];
        const targets =
            agent === undefined
                ? await agentConversations(tx)
                : await conversationsFor(tx, agent);
        for (const { id, place } of targets) {
            if (
                agent !== undefined &&
                (place.hamlet !== agent.hamlet || place.number !== agent.number)
            )
                continue;
            const live = await tx.doc(LiveDoc, id);
            const inbox = await tx.doc(InboxDoc, id);
            if (live.run !== undefined || inbox.items.length > 0)
                places.push({
                    id,
                    agent: { hamlet: place.hamlet, number: place.number },
                    chat: {
                        agent: { hamlet: place.hamlet, number: place.number },
                        chief: place.chief ?? home.chief,
                    },
                });
        }
        return places;
    }, context);
    for (const place of places) {
        const inbox = await home.harness.snapshot(InboxDoc, place.id, context);
        const dropped =
            inbox?.items.filter(
                (item) =>
                    item.mode !== "write" &&
                    item.entry?.kind === ChiefEntry.kind,
            ).length ?? 0;
        const conversation = await home.harness.conversation(place.id, context);
        await home.harness.commit((tx) => cancelAnswers(tx, place.id), context);
        await conversation?.abort(context);
        await home.harness.commit(async (tx) => {
            await lapse(
                tx,
                place.chat,
                { how: "stopped", chief: home.chief },
                home.now(),
            );
            for (const wait of forAgent(await open(tx, context), place.agent)) {
                if (wait.kind !== "fault")
                    await settle(
                        tx,
                        wait.id,
                        { how: "stopped", chief: home.chief, text: "" },
                        home.now(),
                    );
            }
            await notify(
                tx,
                place.id,
                `Stopped: stopped by ${home.chief}${dropped === 0 ? "" : ` (${dropped} queued dropped)`}`,
                home.now(),
            );
        }, context);
    }
    if (agent === undefined) return "Stopping everything.";
    const record = await home.harness.snapshot(
        RecordDoc,
        agent.hamlet,
        context,
    );
    const name =
        record === undefined ? "the agent" : addressOf(record, agent.number);
    return `Stopping ${name}.`;
}

/** Carries a failed or stopped turn on as a new H12 task (C26). */
export async function retry(home: Home, chat: ChatId, context: Context) {
    const conversation = await chatOf(home, chat, context);
    await followHome(home, context);
    const busy = await home.harness.commit(
        async (tx) =>
            (await tx.doc(LiveDoc, conversation.id)).run !== undefined ||
            (await tx.doc(InboxDoc, conversation.id)).items.length > 0,
        context,
    );
    if (busy) throw new Error("A turn is running here. Stop it, then retry.");
    const last = (await conversation.context(context)).messages.at(-1);
    if (
        last === undefined ||
        (last.role === "assistant" &&
            (last.stopReason === "stop" || last.stopReason === "length"))
    )
        throw new Error(
            "the last message was answered, so there is nothing to carry on",
        );
    return conversation.submit(
        { ...retryInput(newTask(home.now()), home.now()), whenBusy: "reject" },
        context,
    );
}

/** A fresh conversation, pointed at immediately while its predecessor finishes (C24). */
export async function newChat(home: Home, chat: ChatId, context: Context) {
    const previous = await chatOf(home, chat, context);
    await followHome(home, context);
    const key = agentKey(chat.agent);
    const id = await home.harness.commit(async (tx) => {
        const known = (await tx.doc(ChatDoc, key, key)).chats[chat.chief];
        if (known === undefined) throw new Error("The chat is gone.");
        const number =
            Math.max(...known.conversations.map((each) => each.number)) + 1;
        const created = await startConversation(home, tx, {
            ...chat.agent,
            chief: chat.chief,
            chat: number,
        });
        known.current = number;
        known.conversations.push({
            number,
            conversation: created,
            began: home.now(),
        });
        await notify(tx, created, "Started a new session.", home.now());
        return created;
    }, context);
    const conversation = await home.harness.conversation(id, context);
    if (conversation === undefined)
        throw new Error("The new conversation is gone.");
    if ((await reflectEvery(home, context)) !== 0) {
        await home.harness.commit(async (tx) => {
            (await tx.doc(ReflectDoc, previous.id)).retiring = true;
        }, context);
        await requestReflection(home, previous.id, context, "owed");
    }
    return conversation;
}

/** Returns to a conversation by its stable number in the chief's chat (C24). */
export async function resumeChat(
    home: Home,
    chat: ChatId,
    number: number,
    context: Context,
) {
    const previous = await chatOf(home, chat, context);
    await followHome(home, context);
    const key = agentKey(chat.agent);
    const id = await home.harness.commit(async (tx) => {
        const known = (await tx.doc(ChatDoc, key, key)).chats[chat.chief];
        const past = known?.conversations.find(
            (each) => each.number === number,
        );
        if (known === undefined || past === undefined)
            throw new Error(`There is no past session \`${number}\` here.`);
        known.current = number;
        (await tx.doc(ReflectDoc, past.conversation)).retiring = false;
        await resetTrust(tx, await tx.doc(WithDoc, past.conversation));
        await configureAgent(home, tx, past.conversation);
        await notify(
            tx,
            past.conversation,
            "Went back to an earlier session.",
            home.now(),
        );
        return currentOf(known);
    }, context);
    if (previous.id !== id && (await reflectEvery(home, context)) !== 0) {
        await home.harness.commit(async (tx) => {
            (await tx.doc(ReflectDoc, previous.id)).retiring = true;
        }, context);
        await requestReflection(home, previous.id, context, "owed");
    }
    return home.harness.conversation(id, context);
}

/** Reflects the current conversation on the chief's request (C5). */
export async function reflect(home: Home, chat: ChatId, context: Context) {
    await followHome(home, context);
    return requestReflection(
        home,
        (await chatOf(home, chat, context)).id,
        context,
        "explicit",
    );
}

/** Takes back a chief line by forking before it. Effects and spent calls are not undone (C27). */
export async function rewind(
    home: Home,
    chat: ChatId,
    item: string,
    context: Context,
): Promise<string> {
    const conversation = await chatOf(home, chat, context);
    await followHome(home, context);
    const entries = await entriesOf(conversation, context);
    const target = entries.findIndex((entry) => `${entry.id}:0` === item);
    const entry = entries[target];
    if (!ChiefEntry.is(entry) || entry.data.chief !== chat.chief)
        throw new Error("That is not a chief's message in this conversation.");
    const kept = entries.slice(0, target);
    let turns = 0;
    for (const each of kept) {
        if (ReflectionEntry.is(each)) turns = 0;
        else if (ChiefEntry.is(each) || RetryEntry.is(each)) turns++;
    }
    const key = agentKey(chat.agent);
    await home.harness.commit(async (tx) => {
        const live = await tx.doc(LiveDoc, conversation.id);
        const inbox = await tx.doc(InboxDoc, conversation.id);
        const reflection = await tx.doc(ReflectDoc, conversation.id);
        if (
            live.run !== undefined ||
            inbox.items.length > 0 ||
            (live.compactions?.length ?? 0) > 0 ||
            reflection.pending.length > 0
        )
            throw new Error("A turn is running here. Stop it, then rewind.");
        const known = (await tx.doc(ChatDoc, key, key)).chats[chat.chief];
        const current = known?.conversations.find(
            (each) => each.number === known.current,
        );
        if (known === undefined || current?.conversation !== conversation.id)
            throw new Error("The current conversation changed. Try again.");
        const count = Math.min(
            (await tx.doc(ReflectionDoc, conversation.id)).unreflected,
            turns,
        );
        await lapse(
            tx,
            chat,
            {
                how: "rewound",
                chief: home.chief,
                since: entry.data.at,
                conversation: current.number,
            },
            home.now(),
        );
        const previous = kept.at(-1);
        const fork =
            previous === undefined
                ? await startConversation(home, tx, {
                      ...chat.agent,
                      chief: chat.chief,
                      chat: current.number,
                  })
                : (
                      await tx.forkConversation(conversation.id, previous.id, {
                          ownership: { kind: "ownerless" },
                      })
                  ).id;
        const index = await tx.doc(AgentConversationsDoc, key, key);
        if (!index.ids.includes(fork)) index.ids.push(fork);
        await configureAgent(home, tx, fork);
        Object.assign(await tx.doc(WithDoc, fork), {
            ...chat.agent,
            chief: chat.chief,
            chat: current.number,
        });
        (await tx.doc(ReflectionDoc, fork)).unreflected = count;
        (await tx.doc(LineageDoc, fork)).previous = conversation.id;
        current.conversation = fork;
    }, context);
    return entry.data.text;
}

async function agentConversations(
    tx: Tx,
): Promise<{ id: ConversationId; place: With }[]> {
    const result: { id: ConversationId; place: With }[] = [];
    for (const { id } of liveHamlets(await tx.doc(RealmDoc))) {
        for (const agent of (await tx.doc(RecordDoc, id, id)).agents) {
            result.push(
                ...(await conversationsFor(tx, {
                    hamlet: id,
                    number: agent.number,
                })),
            );
        }
    }
    return result;
}

async function conversationsFor(
    tx: Tx,
    agent: AgentRef,
): Promise<{ id: ConversationId; place: With }[]> {
    const key = agentKey(agent);
    const result: { id: ConversationId; place: With }[] = [];
    for (const id of (await tx.doc(AgentConversationsDoc, key, key)).ids)
        result.push({ id, place: await tx.doc(WithDoc, id) });
    return result;
}
