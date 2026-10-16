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
} from "./chats/conversations.ts";
import { PayerDoc } from "./chats/errands.ts";
import { TaskDoc } from "./chats/tasks.ts";
import { turnOf } from "./chats/turn.ts";
import { untilAborted } from "./home/abort.ts";
import type { Home, Part } from "./home.ts";
import { PeersDoc } from "./letters/peers.ts";
import { LimitsDoc } from "./limits/rules.ts";
import { liveHamlets, RealmDoc, type RealmEnd } from "./realm/places.ts";
import { carryRealmRoad, realmRoads, resolve } from "./realm/roads.ts";
import {
    type AgentRef,
    addressOf,
    agentKey,
    carryRoad,
    type DecisionHeld,
    type HamletRecord,
    hasRoad,
    memoised,
    RecordDoc,
    type RecordRefusal,
    recordRefusal,
    toolText as recordToolText,
} from "./record.ts";
import { err, ok, type Result } from "./result.ts";
import { safeWords } from "./scrub.ts";

/** A refused letter or question, with its safe address and replay context. */
export type LetterFailure = { operation: "letters"; retrySafe: false } & (
    | {
          kind:
              | "letter-no-place"
              | "letter-waiting"
              | "letter-mayor-road"
              | "letter-empty"
              | "letter-departed"
              | "letter-no-road";
      }
    | { kind: "letter-cross-hamlet" | "letter-unreachable"; name: string }
    | { kind: "letter-messages-full"; most: number }
    | { kind: "letter-unfinished"; address: string; text: string }
);
function letterRefusal<
    const T extends Omit<LetterFailure, "operation" | "retrySafe">,
>(reason: T): Result<never, T & { operation: "letters"; retrySafe: false }> {
    return err({ ...reason, operation: "letters", retrySafe: false });
}
/** Agent-facing recovery words for letters and questions. */
export function letterFailureText(error: LetterFailure): string {
    switch (error.kind) {
        case "letter-no-place":
            return "not in a hamlet";
        case "letter-waiting":
            return "another agent is waiting on your reply, so you cannot wait for a later one. Use ask_agent.";
        case "letter-cross-hamlet":
            return `\`${error.name}\` lives in another hamlet; use ask_agent to reach it`;
        case "letter-mayor-road":
            return "The mayor builds roads with build_road.";
        case "letter-empty":
            return "`message` is empty";
        case "letter-departed":
            return "The agent has left.";
        case "letter-no-road":
            return "There is no road between those agents.";
        case "letter-unreachable":
            return `you have no road to \`${error.name}\`. Ask the mayor to pass a single message on, or ask it for a road with request_road.`;
        case "letter-messages-full":
            return `this task has used all ${error.most} of its messages between agents; finish with what you have and report. The chief can raise it under One Task on the hamlet's Bylaws page`;
        case "letter-unfinished":
            return `\`${error.address}\` could not finish: ${error.text}`;
    }
    const unhandled: never = error;
    throw new Error(`Unclassified letter refusal: ${unhandled}`);
}
function toolText(error: LetterFailure | RecordRefusal | DecisionHeld): string {
    switch (error.kind) {
        case "letter-no-place":
        case "letter-waiting":
        case "letter-cross-hamlet":
        case "letter-mayor-road":
        case "letter-empty":
        case "letter-departed":
        case "letter-no-road":
        case "letter-unreachable":
        case "letter-messages-full":
        case "letter-unfinished":
            return letterFailureText(error);
        default:
            return recordToolText(error);
    }
}

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
                                return ok(
                                    `Sent. \`${known.address}\` is working on it; its reply will come to you as a new message.`,
                                );
                            const place = await api.snapshot(
                                WithDoc,
                                api.conversationId,
                                context,
                            );
                            if (!place)
                                return letterRefusal({
                                    kind: "letter-no-place",
                                });
                            const resolved = await targetOf(
                                api,
                                place,
                                name,
                                context,
                            );
                            if (!resolved.ok) return resolved;
                            const target = resolved.value;
                            const turn = await turnOf(
                                home.harness,
                                api,
                                context,
                            );
                            if (turn?.kind === QuestionEntry.kind)
                                return letterRefusal({
                                    kind: "letter-waiting",
                                });
                            if (target.ref.hamlet !== place.hamlet)
                                return letterRefusal({
                                    kind: "letter-cross-hamlet",
                                    name,
                                });
                            const prepared = await prepare(
                                home,
                                api,
                                target.ref,
                                message,
                                "letters",
                                context,
                            );
                            if (!prepared.ok) return prepared;
                            return ok(
                                `Sent. \`${prepared.value.address}\` is working on it; its reply will come to you as a new message.`,
                            );
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
                            if (known) {
                                const answer = await askAgent(
                                    home,
                                    api,
                                    known.to,
                                    message,
                                    context,
                                );
                                return answer.ok
                                    ? ok(
                                          `\`${known.address}\`: ${answer.value}`,
                                      )
                                    : answer;
                            }
                            const place = await api.snapshot(
                                WithDoc,
                                api.conversationId,
                                context,
                            );
                            if (!place)
                                return letterRefusal({
                                    kind: "letter-no-place",
                                });
                            const resolved = await targetOf(
                                api,
                                place,
                                name,
                                context,
                            );
                            if (!resolved.ok) return resolved;
                            const target = resolved.value;
                            const answer = await askAgent(
                                home,
                                api,
                                target.ref,
                                message,
                                context,
                            );
                            return answer.ok
                                ? ok(`\`${target.address}\`: ${answer.value}`)
                                : answer;
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
                            if (!place)
                                return letterRefusal({
                                    kind: "letter-no-place",
                                });
                            const record = await api.snapshot(
                                RecordDoc,
                                place.hamlet,
                                context,
                            );
                            if (!record || place.number === record.mayor)
                                return letterRefusal({
                                    kind: "letter-mayor-road",
                                });
                            const resolved = await targetOf(
                                api,
                                place,
                                to,
                                context,
                            );
                            if (!resolved.ok) return resolved;
                            const target = resolved.value;
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
                                return ok(
                                    `You already have a road to \`${to}\`.`,
                                );
                            const me = addressOf(record, place.number);
                            const answer = await askAgent(
                                home,
                                api,
                                { hamlet: place.hamlet, number: record.mayor },
                                `\`${me}\` asks for a road to \`${to}\`. Its reason: ${reason}\nDecide. To agree, call build_road with your reason, or agree_road if \`${to}\` lives in another hamlet. To refuse, say why in a sentence.`,
                                context,
                            );
                            if (!answer.ok) return answer;
                            const reply = answer.value;
                            if (
                                await connected(api, place, target.ref, context)
                            )
                                return ok(
                                    `The mayor built the road. You can now ask_agent \`${to}\`.`,
                                );
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
                            return ok(`The mayor did not build it: ${reply}`);
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

async function words(
    run: () => Promise<Result<string, LetterFailure | RecordRefusal>>,
) {
    const result = await run();
    return {
        output: [
            {
                type: "text" as const,
                text: result.ok ? result.value : toolText(result.error),
            },
        ],
    };
}
async function targetOf(
    read: DocumentReader,
    place: AgentRef,
    name: string,
    context: Context,
) {
    const target = await resolve(read, place.hamlet, name, context);
    if (!target)
        return recordRefusal("letters", place.hamlet, {
            kind: "agent-missing",
            name,
        });
    if ("gone" in target)
        return recordRefusal("letters", place.hamlet, {
            kind: "agent-departed",
            farewell: target.gone,
        });
    return ok(target);
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
): Promise<Result<void, LetterFailure>> {
    const doc = await tx.doc(TaskDoc, task, task);
    const limits = await tx.doc(LimitsDoc, from.hamlet, from.hamlet);
    if (!reply && doc.messages >= limits.task.messages)
        return letterRefusal({
            kind: "letter-messages-full",
            most: limits.task.messages,
        });
    if (from.hamlet === to.hamlet) {
        const record = await tx.doc(RecordDoc, from.hamlet, from.hamlet);
        if (hasRoad(record, from.number, to.number)) {
            carryRoad(record, from.number, to.number, home.now());
        } else if (!reply) return letterRefusal({ kind: "letter-no-road" });
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
        else if (!reply) return letterRefusal({ kind: "letter-no-road" });
    }
    doc.messages++;
    return ok(undefined);
}

async function prepare(
    home: Home,
    api: ToolExecutionApi,
    to: AgentRef,
    text: string,
    kind: "letters" | "questions",
    context: Context,
): Promise<Result<Delivery, LetterFailure>> {
    const saved = (await api.snapshot(SentDoc, api.taskId, context))?.delivery;
    if (saved) return ok(saved);
    if (!text.trim()) return letterRefusal({ kind: "letter-empty" });
    const from = await api.snapshot(WithDoc, api.conversationId, context);
    if (!from) return letterRefusal({ kind: "letter-no-place" });
    const record = await api.snapshot(RecordDoc, to.hamlet, context);
    const name = record && addressOf(record, to.number);
    if (!name) return letterRefusal({ kind: "letter-departed" });
    if (!(await connected(api, from, to, context)))
        return letterRefusal({ kind: "letter-unreachable", name });
    const task =
        (await turnOf(home.harness, api, context))?.task ??
        `message:${api.taskId}`;
    return memoised(
        api,
        {
            read: async (tx) => {
                const saved = (await tx.doc(SentDoc, api.taskId)).delivery;
                return saved
                    ? ok({
                          ...saved,
                          from: { ...saved.from },
                          to: { ...saved.to },
                      })
                    : undefined;
            },
            change: async (tx) => {
                const carried = await carry(home, tx, from, to, task, false);
                if (!carried.ok) return carried;
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
                    address:
                        from.hamlet === to.hamlet
                            ? name
                            : `${name}@${to.hamlet}`,
                    task,
                    text: safeWords(home, text),
                    at: home.now(),
                };
                if (kind === "letters")
                    (await tx.doc(PendingDoc)).letters.push({
                        ...delivery,
                        counted: false,
                    });
                return ok(delivery);
            },
            write: async (tx, value) => {
                if (value.ok)
                    (await tx.doc(SentDoc, api.taskId)).delivery = value.value;
            },
        },
        context,
    );
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
): Promise<Result<string, { kind: "answer-unfinished"; text: string }>> {
    const settled = await (await home.harness.submission(id, context))?.wait(
        context,
    );
    if (settled?.type !== "input" || settled.status !== "done")
        return err({
            kind: "answer-unfinished",
            text: safeWords(
                home,
                settled && "reason" in settled
                    ? settled.reason
                    : "It could not finish.",
            ),
        });
    const entry = await home.harness.commit(
        (tx) => tx.entry(AssistantEntry, settled.answer),
        context,
    );
    const message = entry?.model?.[0];
    return ok(
        safeWords(
            home,
            message?.role === "assistant"
                ? message.content
                      .flatMap((p) => (p.type === "text" ? [p.text] : []))
                      .join("\n")
                : "",
        ),
    );
}

/** A question to an agent, with its answer. */
export async function askAgent(
    home: Home,
    api: ToolExecutionApi,
    to: AgentRef,
    text: string,
    context: Context,
): Promise<Result<string, LetterFailure>> {
    const prepared = await prepare(home, api, to, text, "questions", context);
    if (!prepared.ok) return prepared;
    const sent = prepared.value;
    const admitted = await admitQuestion(home, api, sent, context);
    if (!admitted.ok) {
        switch (admitted.error.kind) {
            case "question-cycle":
                return ok(
                    `\`${admitted.error.address}\` is waiting on your answer through this chain of questions. Answer it in your reply instead of asking it again.`,
                );
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
    const carried = await api.commit(async (tx) => {
        const sentDoc = await tx.doc(SentDoc, api.taskId);
        if (!sentDoc.answered) {
            const result = await carry(
                home,
                tx,
                sent.to,
                sent.from,
                sent.task,
                true,
            );
            if (!result.ok) return result;
            sentDoc.answered = true;
        }
        return ok(undefined);
    }, context);
    if (!carried.ok) return carried;
    if (!answer.ok)
        return letterRefusal({
            kind: "letter-unfinished",
            address: sent.address,
            text: answer.error.text,
        });
    return ok(answer.value);
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
            content: `Reply from \`${sent.address}\`: ${answer.ok ? answer.value : `it could not finish: ${answer.error.text}`}`,
            entry: {
                kind: ReplyEntry.kind,
                data: { from: sent.to, task: sent.task, at: home.now() },
            },
        },
        context,
    );
    await forgetLetter(home, sent.id, context);
}

export { conversationViews, PeersDoc } from "./letters/peers.ts";
export { PeerStatistics } from "./letters/statistics.ts";
