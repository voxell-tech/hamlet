import type { Context } from "@earendil-works/chord";
import type { Submission } from "@earendil-works/pi-durable";
import {
    type ConversationId,
    InboxDoc,
    LiveDoc,
} from "@earendil-works/pi-durable";
import { configureAgent } from "../chats/configure.ts";
import type { Home } from "../home.ts";
import { resetTrust } from "../oversight/asked.ts";
import { lapse, OversightDoc, settle } from "../oversight/waits.ts";
import { chiefAccess } from "../realm/access.ts";
import { RealmDoc } from "../realm/places.ts";
import { type AgentRef, addressOf, agentKey, RecordDoc } from "../record.ts";
import { err, ok, type Result } from "../result.ts";
import type { Receipt } from "../server.ts";
import { cancelAnswers } from "./answers.ts";
import {
    AgentConversationsDoc,
    ChatDoc,
    type ChatId,
    ChiefEntry,
    current,
    currentOf,
    past,
    ReflectionEntry,
    RetryEntry,
    retryInput,
    startConversation,
    WithDoc,
} from "./conversations.ts";
import type { ConversationFailure } from "./failure.ts";
import { entriesOf, LineageDoc } from "./history.ts";
import { notify } from "./notice.ts";
import { agentConversations, conversationsFor } from "./owned.ts";
import {
    ReflectDoc,
    ReflectionDoc,
    reflectEvery,
    requestReflection,
} from "./reflection.ts";
import { newTask } from "./turn.ts";

const STARTED = "Started a new session.";
const RESUMED = "Went back to an earlier session.";

/** Stops every agent of the living hamlets this chief leads. */
async function stopChiefAgents(home: Home, context: Context): Promise<string> {
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
            for (const wait of Object.values(
                (await tx.doc(OversightDoc)).waits,
            )) {
                if (
                    wait.kind !== "fault" &&
                    wait.agent?.hamlet === place.agent.hamlet &&
                    wait.agent.number === place.agent.number
                )
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
export async function carryOn(
    home: Home,
    chat: ChatId,
    context: Context,
    submitted?: (submission: Submission) => void,
): Promise<Result<Receipt, ConversationFailure>> {
    const conversation = await current(home, chat, context);
    await home.follow.settled(context);
    const busy = await home.harness.commit(
        async (tx) =>
            (await tx.doc(LiveDoc, conversation.id)).run !== undefined ||
            (await tx.doc(InboxDoc, conversation.id)).items.length > 0,
        context,
    );
    if (busy)
        return err({ kind: "busy", operation: "retry", chat, retrySafe: true });
    const last = (await conversation.context(context)).messages.at(-1);
    if (
        last === undefined ||
        (last.role === "assistant" &&
            (last.stopReason === "stop" || last.stopReason === "length"))
    )
        return err({
            kind: "nothing-to-carry-on",
            operation: "retry",
            chat,
            retrySafe: true,
        });
    const submission = await conversation.submit(
        { ...retryInput(newTask(home.now()), home.now()), whenBusy: "reject" },
        context,
    );
    submitted?.(submission);
    return ok({ text: "Carrying on from where it stopped." });
}

/** A fresh conversation, pointed at immediately while its predecessor finishes (C24). */
export async function start(
    home: Home,
    chat: ChatId,
    context: Context,
): Promise<Result<Receipt, ConversationFailure>> {
    const previous = await current(home, chat, context);
    await home.follow.settled(context);
    const key = agentKey(chat.agent);
    const id = await home.harness.commit(async (tx) => {
        const known = (await tx.doc(ChatDoc, key, key)).chats[chat.chief];
        if (known === undefined)
            return err({
                kind: "chat-gone",
                operation: "new",
                chat,
                retrySafe: false,
            } as const);
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
        await notify(tx, created, STARTED, home.now());
        return ok(created);
    }, context);
    if (!id.ok) return id;
    const conversation = await home.harness.conversation(id.value, context);
    if (conversation === undefined)
        throw new Error("The new conversation is gone.");
    if ((await reflectEvery(home, context)) !== 0) {
        await home.harness.commit(async (tx) => {
            (await tx.doc(ReflectDoc, previous.id)).retiring = true;
        }, context);
        await requestReflection(home, previous.id, context, "owed");
    }
    return ok({ text: STARTED });
}

/** Returns to a conversation by its stable number in the chief's chat (C24). */
export async function resume(
    home: Home,
    chat: ChatId,
    number: number,
    context: Context,
): Promise<Result<Receipt, ConversationFailure>> {
    const selected = await past(home, chat, number, context);
    if (!selected.ok) return selected;
    const previous = await current(home, chat, context);
    await home.follow.settled(context);
    const key = agentKey(chat.agent);
    const id = await home.harness.commit(async (tx) => {
        const known = (await tx.doc(ChatDoc, key, key)).chats[chat.chief];
        const past = known?.conversations.find(
            (each) => each.number === number,
        );
        if (known === undefined || past === undefined)
            return err({
                kind: "no-past-session",
                operation: "resume",
                chat,
                number,
                retrySafe: true,
            } as const);
        known.current = number;
        (await tx.doc(ReflectDoc, past.conversation)).retiring = false;
        await resetTrust(tx, await tx.doc(WithDoc, past.conversation));
        await configureAgent(home, tx, past.conversation);
        await notify(tx, past.conversation, RESUMED, home.now());
        return ok(currentOf(known));
    }, context);
    if (!id.ok) return id;
    if (previous.id !== id.value && (await reflectEvery(home, context)) !== 0) {
        await home.harness.commit(async (tx) => {
            (await tx.doc(ReflectDoc, previous.id)).retiring = true;
        }, context);
        await requestReflection(home, previous.id, context, "owed");
    }
    return ok({ text: RESUMED });
}

/** Reflects the current conversation on the chief's request (C5). */
export async function reflect(
    home: Home,
    chat: ChatId,
    context: Context,
    submitted?: (submission: Submission) => void,
): Promise<Result<Receipt, ConversationFailure>> {
    await home.follow.settled(context);
    const submission = await requestReflection(
        home,
        (await current(home, chat, context)).id,
        context,
        "explicit",
    );
    if (submission) submitted?.(submission);
    return ok({ text: "" });
}

/** Takes back a chief line by forking before it. Effects and spent calls are not undone (C27). */
export async function takeBack(
    home: Home,
    chat: ChatId,
    item: string,
    context: Context,
): Promise<Result<Receipt, ConversationFailure>> {
    const conversation = await current(home, chat, context);
    await home.follow.settled(context);
    const entries = await entriesOf(conversation, context);
    const target = entries.findIndex((entry) => `${entry.id}:0` === item);
    const entry = entries[target];
    if (!ChiefEntry.is(entry) || entry.data.chief !== chat.chief)
        return err({
            kind: "not-chief-line",
            operation: "rewind",
            chat,
            retrySafe: true,
        });
    const kept = entries.slice(0, target);
    let turns = 0;
    for (const each of kept) {
        if (ReflectionEntry.is(each)) turns = 0;
        else if (ChiefEntry.is(each) || RetryEntry.is(each)) turns++;
    }
    const key = agentKey(chat.agent);
    const outcome = await home.harness.commit(async (tx) => {
        const live = await tx.doc(LiveDoc, conversation.id);
        const inbox = await tx.doc(InboxDoc, conversation.id);
        const reflection = await tx.doc(ReflectDoc, conversation.id);
        if (
            live.run !== undefined ||
            inbox.items.length > 0 ||
            (live.compactions?.length ?? 0) > 0 ||
            reflection.pending.length > 0
        )
            return err({
                kind: "busy",
                operation: "rewind",
                chat,
                retrySafe: true,
            } as const);
        const known = (await tx.doc(ChatDoc, key, key)).chats[chat.chief];
        const current = known?.conversations.find(
            (each) => each.number === known.current,
        );
        if (known === undefined || current?.conversation !== conversation.id)
            return err({
                kind: "conversation-changed",
                operation: "rewind",
                chat,
                retrySafe: true,
            } as const);
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
        return ok({ text: entry.data.text, kind: "rewound" } as const);
    }, context);
    return outcome;
}

/** Stops one agent or every agent this chief leads, before acknowledging the request. */
export async function stop(
    home: Home,
    agent: AgentRef | "all",
    context: Context,
): Promise<Result<Receipt, ConversationFailure>> {
    return ok({
        text:
            agent === "all"
                ? await stopChiefAgents(home, context)
                : await stopAgent(home, agent, context),
    });
}
