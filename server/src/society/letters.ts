import type { Context } from "@earendil-works/chord";
import {
    BACKGROUND_CONTEXT,
    withAbortSignal,
} from "@earendil-works/chord/context";
import { Type } from "@earendil-works/pi-ai";
import {
    AssistantEntry,
    type ConversationId,
    type DocumentReader,
    defineDoc,
    defineExtension,
    defineTool,
    GenerationTask,
    hook,
    LiveDoc,
    type SubmissionId,
    type TaskId,
    type ToolExecutionApi,
    type Tx,
} from "@earendil-works/pi-durable";
import {
    LetterEntry,
    QuestionEntry,
    ReplyEntry,
    startConversation,
    WithDoc,
} from "../chats.ts";
import { PayerDoc } from "../errands.ts";
import type { Home, Part } from "../home.ts";
import { untilAborted } from "../home.ts";
import { LimitsDoc } from "../limits.ts";
import {
    carryRealmRoad,
    carryRoad,
    hasRoad,
    liveHamlets,
    RealmDoc,
    type RealmEnd,
    realmRoads,
    resolve,
    roadsOf,
} from "../realm.ts";
import {
    type AgentRef,
    addressOf,
    agentKey,
    type HamletRecord,
    RecordDoc,
} from "../record.ts";
import { safeWords } from "../scrub.ts";
import { TaskDoc } from "../tasks.ts";
import { turnOf } from "../turn.ts";
import { PeersDoc } from "./docs.ts";

type Delivery = {
    id: string;
    conversation: ConversationId;
    replyTo: ConversationId;
    from: AgentRef;
    to: AgentRef;
    address: string;
    task: string;
    text: string;
    at: number;
};
const SentDoc = defineDoc<{
    delivery: Delivery | null;
    answered: boolean;
    refused: boolean;
}>({
    kind: "hamlet.sent",
    version: 1,
    scope: "task",
    initial: () => ({ delivery: null, answered: false, refused: false }),
});
const PendingDoc = defineDoc<{ letters: (Delivery & { counted: boolean })[] }>({
    kind: "hamlet.letters-pending",
    version: 1,
    scope: "session",
    initial: () => ({ letters: [] }),
});

const AskingDoc = defineDoc<{
    edges: {
        call: TaskId;
        from: ConversationId;
        to: ConversationId;
        input: SubmissionId;
    }[];
}>({
    kind: "hamlet.asking",
    version: 1,
    scope: "session",
    initial: () => ({ edges: [] }),
});

type QuestionAdmission =
    | { ok: true; value: null }
    | {
          ok: false;
          error: {
              kind: "question-cycle";
              operation: "ask-agent";
              address: string;
              request: string;
              retrySafe: false;
          };
      };

async function admitQuestion(
    home: Home,
    api: ToolExecutionApi,
    sent: Delivery,
    context: Context,
): Promise<QuestionAdmission> {
    const turn = await turnOf(home.harness, api, context);
    if (!turn) throw new Error("A question needs a running turn.");
    return api.commit(async (tx) => {
        const graph = await tx.doc(AskingDoc);
        const active = [];
        for (const edge of graph.edges) {
            const live = await tx.doc(LiveDoc, edge.from);
            if (live.run?.inputs.includes(edge.input)) active.push(edge);
        }
        graph.edges = active;
        if (graph.edges.some((e) => e.call === api.taskId))
            return { ok: true, value: null };
        const pending = [sent.conversation];
        const seen = new Set<ConversationId>();
        while (pending.length) {
            const id = pending.pop();
            if (id === undefined || seen.has(id)) continue;
            if (id === api.conversationId)
                return {
                    ok: false,
                    error: {
                        kind: "question-cycle",
                        operation: "ask-agent",
                        address: sent.address,
                        request: sent.id,
                        retrySafe: false,
                    },
                };
            seen.add(id);
            pending.push(
                ...graph.edges.filter((e) => e.from === id).map((e) => e.to),
            );
        }
        graph.edges.push({
            call: api.taskId,
            from: api.conversationId,
            to: sent.conversation,
            input: turn.input,
        });
        return { ok: true, value: null };
    }, context);
}

/** The letters and questions along roads (H30, H12). */
export const Letters: Part = (home) => {
    const controller = new AbortController();
    const running = new Map<string, Promise<void>>();
    let unsubscribe: (() => void) | undefined;
    let scheduled = Promise.resolve();
    let stopped = false;
    const discover = async (context: Context) => {
        if (stopped) return;
        for (const letter of (await home.harness.snapshot(PendingDoc, context))
            ?.letters ?? []) {
            if (running.has(letter.id)) continue;
            const work = deliverReply(
                home,
                letter,
                withAbortSignal(controller.signal, context),
            )
                .catch((error: unknown) => {
                    if (!stopped)
                        process.stderr.write(
                            `hamlet: could not return a letter: ${String(error)}\n`,
                        );
                })
                .finally(() => running.delete(letter.id));
            running.set(letter.id, work);
        }
    };
    const schedule = (context: Context) => {
        scheduled = scheduled.then(() => discover(context)).catch(() => {});
    };
    return {
        extension: defineExtension({
            name: "hamlet.letters",
            hooks: [
                hook(GenerationTask, {
                    beforeRequest: async (_request, api, context) => {
                        const turn = await turnOf(home.harness, api, context);
                        if (
                            turn?.kind !== LetterEntry.kind &&
                            turn?.kind !== QuestionEntry.kind &&
                            turn?.kind !== ReplyEntry.kind
                        )
                            return undefined;
                        if (
                            await deliveryAgent(
                                home,
                                api.conversationId,
                                context,
                            )
                        )
                            return undefined;
                        const live = await api.snapshot(
                            LiveDoc,
                            api.conversationId,
                            context,
                        );
                        if (live?.run)
                            void home.harness
                                .abortTask(live.run.taskId, BACKGROUND_CONTEXT)
                                .catch(() => {});
                        return untilAborted(context);
                    },
                }),
            ],
            tools: [
                defineTool({
                    name: "tell_agent",
                    description:
                        "Send a letter along a road. Its reply comes as a new message.",
                    parameters: Type.Object({
                        name: Type.String(),
                        message: Type.String(),
                    }),
                    replay: "safe",
                    execute: async ({ name, message }, api, context) =>
                        words(async () => {
                            const known = (
                                await api.snapshot(SentDoc, api.taskId, context)
                            )?.delivery;
                            if (known)
                                return `Sent. \`${known.address}\` is working on it; its reply will come to you as a new message.`;
                            const place = await api.snapshot(
                                WithDoc,
                                api.conversationId,
                                context,
                            );
                            if (!place) throw new Error("not in a hamlet");
                            const target = await targetOf(
                                api,
                                place,
                                name,
                                context,
                            );
                            const turn = await turnOf(
                                home.harness,
                                api,
                                context,
                            );
                            if (turn?.kind === QuestionEntry.kind)
                                throw new Error(
                                    "another agent is waiting on your reply, so you cannot wait for a later one. Use ask_agent.",
                                );
                            if (target.ref.hamlet !== place.hamlet)
                                throw new Error(
                                    `\`${name}\` lives in another hamlet; use ask_agent to reach it`,
                                );
                            const sent = await prepare(
                                home,
                                api,
                                target.ref,
                                message,
                                "letters",
                                context,
                            );
                            return `Sent. \`${sent.address}\` is working on it; its reply will come to you as a new message.`;
                        }),
                }),
                defineTool({
                    name: "ask_agent",
                    description:
                        "Ask an agent along a road and wait for its answer.",
                    parameters: Type.Object({
                        name: Type.String(),
                        message: Type.String(),
                    }),
                    replay: "safe",
                    execute: async ({ name, message }, api, context) =>
                        words(async () => {
                            const known = (
                                await api.snapshot(SentDoc, api.taskId, context)
                            )?.delivery;
                            if (known)
                                return `\`${known.address}\`: ${await askAgent(home, api, known.to, message, context)}`;
                            const place = await api.snapshot(
                                WithDoc,
                                api.conversationId,
                                context,
                            );
                            if (!place) throw new Error("not in a hamlet");
                            const target = await targetOf(
                                api,
                                place,
                                name,
                                context,
                            );
                            const answer = await askAgent(
                                home,
                                api,
                                target.ref,
                                message,
                                context,
                            );
                            return `\`${target.address}\`: ${answer}`;
                        }),
                }),
                defineTool({
                    name: "request_road",
                    description:
                        "Ask the mayor to build a road to another agent.",
                    parameters: Type.Object({
                        to: Type.String(),
                        reason: Type.String(),
                    }),
                    replay: "safe",
                    execute: async ({ to, reason }, api, context) =>
                        words(async () => {
                            const place = await api.snapshot(
                                WithDoc,
                                api.conversationId,
                                context,
                            );
                            if (!place) throw new Error("not in a hamlet");
                            const record = await api.snapshot(
                                RecordDoc,
                                place.hamlet,
                                context,
                            );
                            if (!record || place.number === record.mayor)
                                throw new Error(
                                    "The mayor builds roads with build_road.",
                                );
                            const target = await targetOf(
                                api,
                                place,
                                to,
                                context,
                            );
                            const known = (
                                await api.snapshot(SentDoc, api.taskId, context)
                            )?.delivery;
                            if (
                                !known &&
                                (await connected(
                                    api,
                                    place,
                                    target.ref,
                                    context,
                                ))
                            )
                                return `You already have a road to \`${to}\`.`;
                            const me = addressOf(record, place.number);
                            const reply = await askAgent(
                                home,
                                api,
                                { hamlet: place.hamlet, number: record.mayor },
                                `\`${me}\` asks for a road to \`${to}\`. Its reason: ${reason}\nDecide. To agree, call build_road with your reason, or agree_road if \`${to}\` lives in another hamlet. To refuse, say why in a sentence.`,
                                context,
                            );
                            if (
                                await connected(api, place, target.ref, context)
                            )
                                return `The mayor built the road. You can now ask_agent \`${to}\`.`;
                            await api.commit(async (tx) => {
                                const record = await tx.doc(
                                    RecordDoc,
                                    place.hamlet,
                                    place.hamlet,
                                );
                                const sent = await tx.doc(SentDoc, api.taskId);
                                if (sent.delivery && !sent.refused) {
                                    record.decisions.push({
                                        kind: "road_refused",
                                        from: place.number,
                                        to,
                                        reason: [...reply]
                                            .slice(0, 160)
                                            .join(""),
                                        by: "mayor",
                                        at: sent.delivery.at,
                                    });
                                    sent.refused = true;
                                }
                            }, context);
                            return `The mayor did not build it: ${reply}`;
                        }),
                }),
            ],
        }),
        async start(context) {
            unsubscribe = home.harness.subscribeCommits(({ changes }) => {
                if (
                    changes.some(
                        (c) =>
                            c.type === "document" &&
                            c.record.kind === PendingDoc.definition.kind,
                    )
                )
                    schedule(context);
            });
            await discover(context);
        },
        async stop() {
            stopped = true;
            unsubscribe?.();
            controller.abort();
            await scheduled;
            await Promise.all(running.values());
        },
    };
};

async function words(run: () => Promise<string>) {
    try {
        return { output: [{ type: "text" as const, text: await run() }] };
    } catch (error) {
        return {
            output: [
                {
                    type: "text" as const,
                    text:
                        error instanceof Error ? error.message : String(error),
                },
            ],
        };
    }
}

async function targetOf(
    read: DocumentReader,
    place: AgentRef,
    name: string,
    context: Context,
) {
    const target = await resolve(read, place.hamlet, name, context);
    if (!target) throw new Error(`no agent named \`${name}\``);
    if ("gone" in target) throw new Error(target.gone);
    return target;
}

async function connected(
    read: DocumentReader,
    from: AgentRef,
    to: AgentRef,
    context: Context,
): Promise<boolean> {
    if (from.hamlet === to.hamlet) {
        const record = await read.snapshot(RecordDoc, from.hamlet, context);
        return !!record && hasRoad(record, from.number, to.number);
    }
    const realm = await read.snapshot(RealmDoc, context);
    if (!realm) return false;
    const records: Record<string, Readonly<HamletRecord>> = {};
    for (const h of liveHamlets(realm)) {
        const record = await read.snapshot(RecordDoc, h.id, context);
        if (record) records[h.id] = record;
    }
    return realmRoads(realm, records).some(
        (r) =>
            (same(r.a, from) && same(r.b, to)) ||
            (same(r.b, from) && same(r.a, to)),
    );
}
function same(a: AgentRef, b: AgentRef) {
    return a.hamlet === b.hamlet && a.number === b.number;
}

async function carry(
    home: Home,
    tx: Tx,
    from: AgentRef,
    to: AgentRef,
    task: string,
    reply: boolean,
) {
    const doc = await tx.doc(TaskDoc, task, task);
    const limits = await tx.doc(LimitsDoc, from.hamlet, from.hamlet);
    if (!reply && doc.messages >= limits.task.messages)
        throw new Error(
            `this task has used all ${limits.task.messages} of its messages between agents; finish with what you have and report. The chief can raise it under One Task on the hamlet's Bylaws page`,
        );
    if (from.hamlet === to.hamlet) {
        const record = await tx.doc(RecordDoc, from.hamlet, from.hamlet);
        if (hasRoad(record, from.number, to.number)) {
            const a = from.number === record.mayor ? "office" : from.number;
            const b = to.number === record.mayor ? "office" : to.number;
            const joins = (r: { a: number | "office"; b: number | "office" }) =>
                (r.a === a && r.b === b) || (r.a === b && r.b === a);
            if (!record.roads.some(joins)) {
                const office = roadsOf(record).find(joins);
                if (office) record.roads.push(office);
            }
            carryRoad(record, from.number, to.number, home.now());
        } else if (!reply)
            throw new Error("There is no road between those agents.");
    } else {
        const realm = await tx.doc(RealmDoc);
        const records: Record<string, HamletRecord> = {};
        for (const h of liveHamlets(realm))
            records[h.id] = await tx.doc(RecordDoc, h.id, h.id);
        const endMatches = (e: RealmEnd, ref: AgentRef) =>
            "office" in e
                ? e.office === ref.hamlet &&
                  records[ref.hamlet]?.mayor === ref.number
                : same(e, ref);
        let road = [...realm.roads, ...realm.offices].find(
            (r) =>
                ((endMatches(r.a, from) && endMatches(r.b, to)) ||
                    (endMatches(r.b, from) && endMatches(r.a, to))) &&
                r.agreed.includes(from.hamlet) &&
                r.agreed.includes(to.hamlet),
        );
        if (
            !road &&
            records[from.hamlet]?.mayor === from.number &&
            records[to.hamlet]?.mayor === to.number
        ) {
            road = {
                a: { office: from.hamlet },
                b: { office: to.hamlet },
                agreed: [from.hamlet, to.hamlet],
                reason: "office",
                messages: 0,
                lastFrom: null,
                lastUsed: 0,
            };
            realm.offices.push(road);
            road = realm.offices.at(-1);
        }
        if (road)
            carryRealmRoad(
                road,
                endMatches(road.a, from) ? road.a : road.b,
                home.now(),
            );
        else if (!reply)
            throw new Error("There is no road between those agents.");
    }
    doc.messages++;
}

async function prepare(
    home: Home,
    api: ToolExecutionApi,
    to: AgentRef,
    text: string,
    kind: "letters" | "questions",
    context: Context,
): Promise<Delivery> {
    const saved = (await api.snapshot(SentDoc, api.taskId, context))?.delivery;
    if (saved) return saved;
    if (!text.trim()) throw new Error("`message` is empty");
    const from = await api.snapshot(WithDoc, api.conversationId, context);
    if (!from) throw new Error("not in a hamlet");
    const record = await api.snapshot(RecordDoc, to.hamlet, context);
    const name = record && addressOf(record, to.number);
    if (!name) throw new Error("The agent has left.");
    if (!(await connected(api, from, to, context)))
        throw new Error(
            `you have no road to \`${name}\`. Ask the mayor to pass a single message on, or ask it for a road with request_road.`,
        );
    const task =
        (await turnOf(home.harness, api, context))?.task ??
        `message:${api.taskId}`;
    return api.commit(async (tx) => {
        const sent = await tx.doc(SentDoc, api.taskId);
        if (sent.delivery) return sent.delivery;
        await carry(home, tx, from, to, task, false);
        const key = agentKey(to);
        const peers = await tx.doc(PeersDoc, key, key);
        const sender = agentKey(from);
        let id = peers[kind][sender];
        if (id === undefined) {
            id = await startConversation(home, tx, {
                ...to,
                chief: null,
                chat: 0,
            });
            peers[kind][sender] = id;
            (await tx.doc(PayerDoc, id)).hamlet = from.hamlet;
        }
        const delivery: Delivery = {
            id: `${kind === "letters" ? "letter" : "ask"}:${api.taskId}`,
            conversation: id,
            replyTo: api.conversationId,
            from: { hamlet: from.hamlet, number: from.number },
            to,
            address: from.hamlet === to.hamlet ? name : `${name}@${to.hamlet}`,
            task,
            text: safeWords(home, text),
            at: home.now(),
        };
        sent.delivery = delivery;
        if (kind === "letters")
            (await tx.doc(PendingDoc)).letters.push({
                ...delivery,
                counted: false,
            });
        return delivery;
    }, context);
}
async function inputOf(
    home: Home,
    sent: Delivery,
    question: boolean,
    context: Context,
) {
    const location = await home.harness.snapshot(
        WithDoc,
        sent.replyTo,
        context,
    );
    const from = location
        ? { hamlet: location.hamlet, number: location.number }
        : sent.from;
    const record = await home.harness.snapshot(RecordDoc, from.hamlet, context);
    const sender =
        (record &&
            (addressOf(record, from.number) ||
                record.archive.findLast((a) => a.agent.number === from.number)
                    ?.agent.name)) ||
        `agent #${from.number}`;
    const address =
        from.hamlet === sent.to.hamlet ? sender : `${sender}@${from.hamlet}`;
    return {
        type: "input" as const,
        requestId: sent.id,
        content: `${question ? "Question" : "Letter"} from \`${address}\`${question ? " (waiting for your answer)" : ""}:\n${sent.text}`,
        entry: question
            ? {
                  kind: QuestionEntry.kind,
                  data: { from, task: sent.task, at: sent.at },
              }
            : {
                  kind: LetterEntry.kind,
                  data: {
                      from,
                      task: sent.task,
                      at: sent.at,
                      replyTo: sent.replyTo,
                  },
              },
    };
}
async function answerOf(
    home: Home,
    id: SubmissionId,
    context: Context,
): Promise<{ ok: boolean; text: string }> {
    const settled = await (await home.harness.submission(id, context))?.wait(
        context,
    );
    if (settled?.type !== "input" || settled.status !== "done")
        return {
            ok: false,
            text: safeWords(
                home,
                settled && "reason" in settled
                    ? settled.reason
                    : "It could not finish.",
            ),
        };
    const entry = await home.harness.commit(
        (tx) => tx.entry(AssistantEntry, settled.answer),
        context,
    );
    const message = entry?.model?.[0];
    return {
        ok: true,
        text: safeWords(
            home,
            message?.role === "assistant"
                ? message.content
                      .flatMap((p) => (p.type === "text" ? [p.text] : []))
                      .join("\n")
                : "",
        ),
    };
}

/** A question to an agent, with its answer. */
export async function askAgent(
    home: Home,
    api: ToolExecutionApi,
    to: AgentRef,
    text: string,
    context: Context,
): Promise<string> {
    const sent = await prepare(home, api, to, text, "questions", context);
    const admitted = await admitQuestion(home, api, sent, context);
    if (!admitted.ok) {
        switch (admitted.error.kind) {
            case "question-cycle":
                return `\`${admitted.error.address}\` is waiting on your answer through this chain of questions. Answer it in your reply instead of asking it again.`;
        }
        const unhandled: never = admitted.error.kind;
        throw new Error(`Unclassified question refusal: ${unhandled}`);
    }
    const conversation = await api.conversation(sent.conversation, context);
    if (!conversation) throw new Error("The question's conversation is gone.");
    const submission = await conversation.submit(
        await inputOf(home, sent, true, context),
        context,
    );
    // The invocation-bound wait is cancelled with this tool, leaving the admitted question durable.
    await submission.wait(context);
    const answer = await answerOf(home, submission.id, context);
    await api.commit(async (tx) => {
        const graph = await tx.doc(AskingDoc);
        graph.edges = graph.edges.filter((e) => e.call !== api.taskId);
    }, context);
    await api.commit(async (tx) => {
        const sentDoc = await tx.doc(SentDoc, api.taskId);
        if (!sentDoc.answered) {
            await carry(home, tx, sent.to, sent.from, sent.task, true);
            sentDoc.answered = true;
        }
    }, context);
    if (!answer.ok)
        throw new Error(`\`${sent.address}\` could not finish: ${answer.text}`);
    return answer.text;
}

async function deliveryAgent(
    home: Home,
    conversation: ConversationId,
    context: Context,
): Promise<AgentRef | null> {
    const place = await home.harness.snapshot(WithDoc, conversation, context);
    if (!place) return null;
    const realm = await home.harness.snapshot(RealmDoc, context);
    if (!liveHamlets(realm).some((h) => h.id === place.hamlet)) return null;
    const record = await home.harness.snapshot(
        RecordDoc,
        place.hamlet,
        context,
    );
    return record?.agents.some((a) => a.number === place.number)
        ? { hamlet: place.hamlet, number: place.number }
        : null;
}

async function forgetLetter(home: Home, id: string, context: Context) {
    await home.harness.commit(async (tx) => {
        const pending = await tx.doc(PendingDoc);
        pending.letters = pending.letters.filter((l) => l.id !== id);
    }, context);
}

async function deliverReply(home: Home, sent: Delivery, context: Context) {
    const to = await deliveryAgent(home, sent.conversation, context);
    if (!to) return forgetLetter(home, sent.id, context);
    sent = { ...sent, to };
    const conversation = await home.harness.conversation(
        sent.conversation,
        context,
    );
    if (!conversation) throw new Error("The letter's conversation is gone.");
    const submission = await conversation.submit(
        await inputOf(home, sent, false, context),
        context,
    );
    const answer = await answerOf(home, submission.id, context);
    const from = await deliveryAgent(home, sent.replyTo, context);
    if (!from) return forgetLetter(home, sent.id, context);
    sent = { ...sent, from };
    const reply = await home.harness.conversation(sent.replyTo, context);
    if (!reply) throw new Error("The sender's conversation is gone.");
    await home.harness.commit(async (tx) => {
        const pending = (await tx.doc(PendingDoc)).letters.find(
            (l) => l.id === sent.id,
        );
        if (pending && !pending.counted) {
            await carry(home, tx, sent.to, sent.from, sent.task, true);
            pending.counted = true;
        }
    }, context);
    await reply.submit(
        {
            type: "input",
            requestId: `reply:${submission.id}`,
            content: `Reply from \`${sent.address}\`: ${answer.ok ? answer.text : `it could not finish: ${answer.text}`}`,
            entry: {
                kind: ReplyEntry.kind,
                data: { from: sent.to, task: sent.task, at: home.now() },
            },
        },
        context,
    );
    await forgetLetter(home, sent.id, context);
}
