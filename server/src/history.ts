import type { Context } from "@earendil-works/chord";
import type { AssistantMessage } from "@earendil-works/pi-ai";
import {
    AssistantEntry,
    CompactionEntry,
    type Conversation,
    type ConversationId,
    type Cursor,
    defineDoc,
    type EntryRecord,
    UsageDoc,
} from "@earendil-works/pi-durable";
import { ChatDoc, type ChatId, ChiefEntry } from "./chats.ts";
import { agentKey } from "./hamlet.ts";
import type { Home } from "./home.ts";
import { micros } from "./money.ts";
import type { Past, UsageView } from "./protocol.ts";
import { usageSummary } from "./usage.ts";

/** The branch put aside by a rewind, including a rewind of the first entry. */
export const LineageDoc = defineDoc<{ previous: ConversationId | null }>({
    kind: "hamlet.lineage",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "initial",
    initial: () => ({ previous: null }),
});

/** The whole visible history, not just the active range after a summary. */
export async function entriesOf(
    conversation: Conversation,
    context: Context,
): Promise<EntryRecord[]> {
    const entries: EntryRecord[] = [];
    let cursor: Cursor | undefined;
    do {
        const page = await conversation.entries(
            { order: "ascending" },
            500,
            cursor,
            context,
        );
        entries.push(...page.items);
        cursor = page.next;
    } while (cursor !== undefined);
    return entries;
}

/** Model usage across the whole fork chain, including branches no longer sent to the model (C27, C31). */
export async function usageOf(
    home: Home,
    id: ConversationId,
    context: Context,
): Promise<UsageView> {
    const chain = await home.harness.commit(async (tx) => {
        const ids: ConversationId[] = [];
        let at: ConversationId | undefined = id;
        while (at !== undefined && !ids.includes(at)) {
            ids.push(at);
            at =
                (await tx.doc(LineageDoc, at)).previous ??
                (await tx.conversation(at))?.parent?.conversationId;
        }
        return ids;
    }, context);
    const usage: UsageView = {
        calls: 0,
        unreported: 0,
        input: 0,
        cached: 0,
        output: 0,
        reasoning: 0,
        ms: 0,
        micros: 0,
        lastInput: 0,
        summary: "",
    };
    for (const part of chain) {
        const conversation = await home.harness.conversation(part, context);
        if (conversation === undefined) continue;
        const entries = (await entriesOf(conversation, context)).filter(
            (entry) => entry.conversationId === part,
        );
        const responses = entries.flatMap((entry): AssistantMessage[] =>
            AssistantEntry.is(entry) && entry.model?.[0]?.role === "assistant"
                ? [entry.model[0]]
                : [],
        );
        for (const message of responses) {
            usage.calls++;
            if (
                message.usage.input +
                    message.usage.cacheRead +
                    message.usage.cacheWrite ===
                    0 &&
                message.usage.output === 0
            )
                usage.unreported++;
            usage.ms += message.durationMs ?? 0;
            usage.micros += micros(
                (message as AssistantMessage & { billedCost?: number })
                    .billedCost ?? message.usage.cost.total,
            );
        }
        const models =
            (await home.harness.snapshot(UsageDoc, part, context))?.models ??
            {};
        let summaryTokens = 0;
        for (const [key, total] of Object.entries(models)) {
            usage.input += total.input + total.cacheRead + total.cacheWrite;
            usage.cached += total.cacheRead;
            usage.output += total.output;
            usage.reasoning += total.reasoning ?? 0;
            const messages = responses.filter(
                (message) => `${message.provider}/${message.model}` === key,
            );
            const input =
                total.input +
                total.cacheRead +
                total.cacheWrite -
                messages.reduce(
                    (sum, message) =>
                        sum +
                        message.usage.input +
                        message.usage.cacheRead +
                        message.usage.cacheWrite,
                    0,
                );
            const output =
                total.output -
                messages.reduce(
                    (sum, message) => sum + message.usage.output,
                    0,
                );
            const tokens = Math.max(0, input + output);
            summaryTokens += tokens;
            usage.micros += micros(
                total.cost.total -
                    messages.reduce(
                        (sum, message) => sum + message.usage.cost.total,
                        0,
                    ),
            );
        }
        const summaries = entries.filter((entry) =>
            CompactionEntry.is(entry),
        ).length;
        usage.calls += summaries;
        if (summaryTokens === 0) usage.unreported += summaries;
    }
    const standing = await (
        await home.harness.conversation(id, context)
    )?.context(context);
    const last = standing?.messages.findLast(
        (message) => message.role === "assistant",
    );
    if (last?.role === "assistant")
        usage.lastInput =
            last.usage.input + last.usage.cacheRead + last.usage.cacheWrite;
    usage.summary = usageSummary(usage);
    return usage;
}

/** Past and current conversations, newest first, with no empty or reflection-only titles (C24). */
export async function historyOf(
    home: Home,
    chat: ChatId,
    context: Context,
): Promise<Past[]> {
    const known = (
        await home.harness.snapshot(ChatDoc, agentKey(chat.agent), context)
    )?.chats[chat.chief];
    const history: Past[] = [];
    for (const past of known?.conversations ?? []) {
        const conversation = await home.harness.conversation(
            past.conversation,
            context,
        );
        if (conversation === undefined) continue;
        const entries = await entriesOf(conversation, context);
        const first = entries.find(
            (entry) => ChiefEntry.is(entry) && entry.data.text.trim() !== "",
        );
        if (!ChiefEntry.is(first)) continue;
        const line =
            first.data.text
                .split("\n")
                .find((text) => text.trim() !== "")
                ?.trim() ?? "";
        const chars = [...line];
        const tokens = await usageOf(home, past.conversation, context);
        history.push({
            number: past.number,
            current: past.number === known?.current,
            title:
                chars.slice(0, 90).join("") + (chars.length > 90 ? "..." : ""),
            by: first.data.chief,
            began: past.began,
            modified: Math.max(
                past.began,
                ...entries.map((entry) =>
                    typeof entry.data === "object" &&
                    entry.data !== null &&
                    !Array.isArray(entry.data) &&
                    typeof entry.data.at === "number"
                        ? entry.data.at
                        : (entry.model?.[0]?.timestamp ?? past.began),
                ),
            ),
            messages: entries.filter(
                (entry) =>
                    ChiefEntry.is(entry) ||
                    (AssistantEntry.is(entry) &&
                        entry.model?.[0]?.role === "assistant" &&
                        entry.model[0].content.some(
                            (part) =>
                                part.type === "text" && part.text.trim() !== "",
                        )),
            ).length,
            tokens: tokens.input + tokens.output,
        });
    }
    return history.sort((a, b) => b.number - a.number);
}
