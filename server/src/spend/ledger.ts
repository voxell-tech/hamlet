import type { Context, JsonRepresentation } from "@earendil-works/chord";
import type { Usage } from "@earendil-works/pi-ai";
import {
    AssistantEntry,
    CompactionEntry,
    type ConversationId,
    type Cursor,
    defineDoc,
    defineDocFamily,
    type EntryId,
    LiveDoc,
    type Tx,
    UsageDoc,
} from "@earendil-works/pi-durable";
import {
    AnswerDoc,
    ChiefEntry,
    ErrandEntry,
    LetterEntry,
    PayerDoc,
    QuestionEntry,
    ReflectionEntry,
    ReplyEntry,
    RetryEntry,
    TaskDoc,
    WithDoc,
} from "../chats.ts";
import type { Home } from "../home.ts";
import { localDay } from "../home.ts";
import type { Limits } from "../limits.ts";
import { type Amount, add, amountOf, micros, zero } from "./money.ts";

/** A model, summary or failed-attempt charge attributed to an agent and task. */
export type Charge = Amount & {
    what: "model" | "summary" | "attempt";
    of: string | null;
    at: number;
    agent: number;
    task: string | null;
    model: string;
    host: string;
    input: number;
    cached: number;
    output: number;
    reasoning: number;
    estimated: boolean;
    stopReason: string | null;
    billed: number | null;
    firstTokenMs: number | null;
};
/** A local-day road charge attributed to its sending agent and task. */
export type RoadCharge = Amount & {
    what: "road";
    at: number;
    agent: number;
    task: string | null;
    of: string | null;
};
/** An append-only spend charge or a chief's reset mark. */
export type LedgerLine =
    | Charge
    | RoadCharge
    | { what: "reset"; at: number; by: string };

/** Append-only charges and reset marks, by hamlet and local day. */
export const LedgerDoc = defineDocFamily<{ lines: LedgerLine[] }, string>({
    kind: "hamlet.ledger",
    version: 2,
    migrate: (value) => ({
        lines: (value as unknown as { lines: LedgerLine[] }).lines.map(
            (line) => (line.what === "reset" ? line : { ...line, of: null }),
        ),
    }),
    scope: "session",
    family: true,
    initial: () => ({ lines: [] }),
});
const DaysDoc = defineDocFamily<{ days: string[] }, string>({
    kind: "hamlet.ledger-days",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ days: [] }),
});
const CountedDoc = defineDoc<{
    through: EntryId | null;
    task: string | null;
    models: Record<string, JsonRepresentation<Usage>>;
}>({
    kind: "hamlet.counted",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "initial",
    initial: () => ({ through: null, task: null, models: {} }),
});

/** Conversations that may hold uncharged usage, marked before a model call can begin. */
const UncountedDoc = defineDoc<{
    conversations: ConversationId[];
    indexed: boolean;
}>({
    kind: "hamlet.uncounted",
    version: 1,
    scope: "session",
    initial: () => ({ conversations: [], indexed: false }),
});

async function append(tx: Tx, hamlet: string, line: LedgerLine): Promise<void> {
    const day = localDay(line.at);
    const days = (await tx.doc(DaysDoc, hamlet, hamlet)).days;
    if (!days.includes(day)) days.push(day);
    (await tx.doc(LedgerDoc, `${hamlet}/${day}`, day)).lines.push(line);
    if (line.what !== "reset" && line.task !== null)
        add((await tx.doc(TaskDoc, line.task, line.task)).spent, line);
}

/** A durable charge reservation before a response or summary can be kept. */
export async function markUncounted(
    home: Home,
    conversation: ConversationId,
    context: Context,
): Promise<void> {
    await home.harness.commit(async (tx) => {
        const uncounted = await tx.doc(UncountedDoc);
        if (!uncounted.conversations.includes(conversation))
            uncounted.conversations.push(conversation);
    }, context);
}

/** Charges newly kept usage in the named conversations, or the write-ahead set, atomically with its receipts. */
export async function reconcile(
    home: Home,
    context: Context,
    conversations?: readonly ConversationId[],
): Promise<void> {
    await home.harness.commit(async (tx) => {
        const uncounted = await tx.doc(UncountedDoc);
        const initial = !uncounted.indexed;
        if (initial) {
            // One catch-up scan for storage created before write-ahead charging.
            let cursor: Cursor | undefined;
            do {
                const page = await tx.scanConversations({}, 100, cursor);
                for (const { id } of page.items)
                    if (!uncounted.conversations.includes(id))
                        uncounted.conversations.push(id);
                cursor = page.next;
            } while (cursor !== undefined);
            uncounted.indexed = true;
        }
        const targets =
            !initial && conversations !== undefined
                ? conversations
                : [...uncounted.conversations];
        for (const id of targets) {
            await count(home, tx, id);
            const live = await tx.doc(LiveDoc, id);
            // An in-flight response may be kept after this count, including across a crash.
            if (live.run === undefined && (live.compactions?.length ?? 0) === 0)
                uncounted.conversations = uncounted.conversations.filter(
                    (each) => each !== id,
                );
        }
    }, context);
}

async function count(
    home: Home,
    tx: Tx,
    conversation: ConversationId,
): Promise<void> {
    const counted = await tx.doc(CountedDoc, conversation);
    const place = await tx.doc(WithDoc, conversation);
    const payer = (await tx.doc(PayerDoc, conversation)).hamlet ?? place.hamlet;
    const of = payer === place.hamlet ? null : place.hamlet;
    const usage = await tx.doc(UsageDoc, conversation);
    const fresh: Record<string, JsonRepresentation<Usage>> = {};
    const recoveries = await tx.doc(AnswerDoc, conversation);
    let summaryTask: string | null | undefined;
    let cursor: Cursor | undefined;
    do {
        const page = await tx.scanEntries(
            {
                conversationId: conversation,
                order: "ascending",
                ...(counted.through === null
                    ? {}
                    : { minEntryId: counted.through }),
            },
            500,
            cursor,
        );
        for (const entry of page.items) {
            if (
                (counted.through !== null && entry.id <= counted.through) ||
                entry.conversationId !== conversation
            )
                continue;
            counted.through = entry.id;
            if (CompactionEntry.is(entry)) {
                const recovery = Object.values(recoveries.marks).find(
                    (mark) => mark.compaction === entry.byTaskId,
                );
                summaryTask = recovery?.task ?? counted.task;
            }
            if (
                ChiefEntry.is(entry) ||
                RetryEntry.is(entry) ||
                ReflectionEntry.is(entry) ||
                LetterEntry.is(entry) ||
                QuestionEntry.is(entry) ||
                ReplyEntry.is(entry) ||
                ErrandEntry.is(entry)
            )
                counted.task = entry.data.task;
            const message = entry.model?.[0];
            if (!AssistantEntry.is(entry) || message?.role !== "assistant")
                continue;
            const key = `${message.provider}/${message.model}`;
            const model = home.models.getModel(message.provider, message.model);
            const failed =
                message.stopReason === "error" ||
                message.stopReason === "aborted";
            const estimated = !failed && message.usage.totalTokens === 0;
            const amount = amountOf(
                message.usage,
                message.durationMs ?? 0,
                model,
                estimated
                    ? ((
                          message as typeof message & {
                              estimatedTokens?: number;
                          }
                      ).estimatedTokens ?? 0)
                    : 0,
                (message as typeof message & { billedCost?: number })
                    .billedCost,
            );
            await append(tx, payer, {
                of,
                ...amount,
                what: failed ? "attempt" : "model",
                at: message.timestamp,
                agent: place.number,
                task: counted.task,
                model: `${message.provider}::${message.model}`,
                host: hostOf(model?.baseUrl, message.provider),
                input:
                    message.usage.input +
                    message.usage.cacheRead +
                    message.usage.cacheWrite,
                cached: message.usage.cacheRead,
                output: message.usage.output,
                reasoning: message.usage.reasoning ?? 0,
                estimated,
                stopReason: message.rawStopReason ?? message.stopReason,
                firstTokenMs:
                    (message as typeof message & { firstTokenMs?: number })
                        .firstTokenMs ?? null,
                billed:
                    (message as typeof message & { billedCost?: number })
                        .billedCost ?? null,
            });
            fresh[key] = sumUsage(fresh[key], message.usage);
        }
        cursor = page.next;
    } while (cursor !== undefined);
    for (const [key, total] of Object.entries(usage.models)) {
        const previous = counted.models[key];
        const entries = fresh[key];
        const extra = difference(total, sumUsage(previous, entries));
        if (extra.totalTokens > 0 || extra.cost.total > 0) {
            const slash = key.indexOf("/");
            const provider = key.slice(0, slash);
            const id = key.slice(slash + 1);
            const model = home.models.getModel(provider, id);
            await append(tx, payer, {
                of,
                ...amountOf(extra, 0, model),
                what: "summary",
                at: home.now(),
                agent: place.number,
                task: summaryTask ?? counted.task,
                model: `${provider}::${id}`,
                host: hostOf(model?.baseUrl, provider),
                input: extra.input,
                cached: extra.cacheRead + extra.cacheWrite,
                output: extra.output,
                reasoning: extra.reasoning ?? 0,
                estimated: false,
                stopReason: null,
                firstTokenMs: null,
                billed: null,
            });
        }
        counted.models[key] = sumUsage(total);
    }
}

function sumUsage(a?: Readonly<Usage>, b?: Readonly<Usage>): Usage {
    return {
        input: (a?.input ?? 0) + (b?.input ?? 0),
        output: (a?.output ?? 0) + (b?.output ?? 0),
        cacheRead: (a?.cacheRead ?? 0) + (b?.cacheRead ?? 0),
        cacheWrite: (a?.cacheWrite ?? 0) + (b?.cacheWrite ?? 0),
        reasoning: (a?.reasoning ?? 0) + (b?.reasoning ?? 0),
        totalTokens: (a?.totalTokens ?? 0) + (b?.totalTokens ?? 0),
        cost: {
            input: 0,
            output: 0,
            cacheRead: 0,
            cacheWrite: 0,
            total: (a?.cost.total ?? 0) + (b?.cost.total ?? 0),
        },
    };
}
function difference(a: Readonly<Usage>, b: Readonly<Usage>): Usage {
    const result = sumUsage(a);
    for (const key of [
        "input",
        "output",
        "cacheRead",
        "cacheWrite",
        "reasoning",
        "totalTokens",
    ] as const)
        result[key] = Math.max(0, (a[key] ?? 0) - (b[key] ?? 0));
    result.cost.total = Math.max(0, a.cost.total - b.cost.total);
    return result;
}
function hostOf(url: string | undefined, fallback: string): string {
    try {
        return url === undefined ? fallback : new URL(url).hostname;
    } catch {
        return fallback;
    }
}

/** Spend after the period's last reset and across the task's whole life. */
export type Books = {
    hamlet: Amount;
    /** Local agents by number, foreign agents by hamlet:number. */
    agents: Record<string, Amount>;
    task: Amount;
};

/** The period's spend after its last reset, and the task's spend for its whole life. */
export async function booksOf(
    home: Home,
    hamlet: string,
    period: "day" | "month",
    task: string | null,
    context: Context,
): Promise<Books> {
    await reconcile(home, context);
    const today = localDay(home.now());
    const first = period === "day" ? today : `${today.slice(0, 8)}01`;
    const books: Books = { hamlet: zero(), agents: {}, task: zero() };
    for (const day of [
        ...((await home.harness.snapshot(DaysDoc, hamlet, context))?.days ??
            []),
    ].sort()) {
        if (day < first || day > today) continue;
        for (const line of (
            await home.harness.snapshot(LedgerDoc, `${hamlet}/${day}`, context)
        )?.lines ?? []) {
            if (line.what === "reset") {
                books.hamlet = zero();
                books.agents = {};
                continue;
            }
            add(books.hamlet, line);
            const agent =
                line.of === null || line.of === hamlet
                    ? String(line.agent)
                    : `${line.of}:${line.agent}`;
            books.agents[agent] ??= zero();
            add(books.agents[agent], line);
        }
    }
    if (task !== null)
        books.task = structuredClone(
            (await home.harness.snapshot(TaskDoc, task, context))?.spent ??
                zero(),
        );
    return books;
}

/** Starts the hamlet's current period again without refunding any task. */
export async function resetBudget(
    home: Home,
    hamlet: string,
    by: string,
    context: Context,
): Promise<string> {
    await reconcile(home, context);
    await home.harness.commit(
        (tx) => append(tx, hamlet, { what: "reset", at: home.now(), by }),
        context,
    );
    return "The budget starts again from here. What was spent before is kept in the ledger.";
}

/** The kept lines of one local day, including calls a rewind took out. */
export async function ledgerOf(
    home: Home,
    hamlet: string,
    day: string,
    context: Context,
): Promise<readonly LedgerLine[]> {
    await reconcile(home, context);
    return (
        (await home.harness.snapshot(LedgerDoc, `${hamlet}/${day}`, context))
            ?.lines ?? []
    );
}

/** The price of a road charged to its hamlet and task (H3). */
export async function chargeRoad(
    tx: Tx,
    hamlet: string,
    given: {
        task: string | null;
        agent: number;
        at: number;
        limits: Readonly<Limits>;
    },
): Promise<void> {
    await append(tx, hamlet, {
        what: "road",
        of: null,
        at: given.at,
        agent: given.agent,
        task: given.task,
        tokens: given.limits.road.tokens,
        micros: micros(given.limits.road.cost),
        ms: given.limits.road.minutes * 60000,
    });
}
