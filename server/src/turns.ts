import type { Context } from "@earendil-works/chord";
import type { AssistantMessage, Message } from "@earendil-works/pi-ai";
import {
    AssistantEntry,
    CompactionEntry,
    type Conversation,
    type ConversationView,
    type Cursor,
    defineExtension,
    type EntryId,
    type EntryRecord,
    hook,
    type InboxState,
    type LiveState,
    ToolResultEntry,
    ToolTask,
    UserEntry,
} from "@earendil-works/pi-durable";
import { SUMMARY_NOTICE } from "./answers.ts";
import { record } from "./catalogue.ts";
import { CHANGE, changeOf } from "./change.ts";
import {
    ChatDoc,
    ChiefEntry,
    chatOf,
    ReflectionEntry,
    RetryEntry,
    WithDoc,
} from "./chats.ts";
import { agentKey } from "./hamlet.ts";
import { usageOf } from "./history.ts";
import type { Home } from "./home.ts";
import { NoticeEntry } from "./notice.ts";
import type {
    ChatRef,
    Item,
    Op,
    ServerMessage,
    Transcript,
    UsageView,
} from "./protocol.ts";
import { addressed } from "./record.ts";
import { Scrubber } from "./scrub.ts";

/** Characters of a tool's arguments a transcript shows (agent/event.rs:10). */
const SUMMARY = 200;

/** Entries read at once while a transcript is built. */
const PAGE = 500;

/** A chief's watch of one chat. */
export type ChatWatch = {
    stop(): Promise<void>;
};

/**
 * Watches `chat` for its chief: sends its transcript as it stands, with `to` when it answers a request, then a `turn`
 * for each step as it happens (C30, S5). Applied in order, the steps give the transcript read again.
 */
export async function watchChat(
    home: Home,
    chat: ChatRef,
    send: (message: ServerMessage) => void,
    to: string | undefined,
    context: Context,
    before?: string,
): Promise<ChatWatch> {
    let stopped = false;
    let watch: Awaited<ReturnType<Conversation["watch"]>> | undefined;
    let id: Conversation["id"] | undefined;
    const bind = async () => {
        const conversation = await conversationOf(home, chat, context);
        if (id === conversation.id || stopped) return;
        await watch?.stop();
        if (stopped) return;
        if (id !== undefined)
            send({ type: "turn", chat, ops: [{ op: "reset" }] });
        id = conversation.id;
        watch = await conversation.watch(context);
        if (stopped) {
            await watch.stop();
            return;
        }
        const number = (
            await home.harness.snapshot(WithDoc, conversation.id, context)
        )?.chat;
        let fold = await readBack(home, conversation, watch.value, context);
        let usage = await usageOf(home, conversation.id, context);
        if (stopped) return;
        send({
            ...transcriptOf(chat, fold, usage, before),
            ...(number === undefined ? {} : { number }),
            ...(to === undefined ? {} : { to }),
        });
        to = undefined;
        before = undefined;
        watch.start(async (value, updates) => {
            const ops = fold.advance(value);
            const replaced = updates.some((op) => op[0] === "r");
            if (fold.stale || replaced) {
                ops.push({ op: "reset" });
                fold = await readBack(home, conversation, value, context);
            }
            const next = await usageOf(home, conversation.id, context);
            if (JSON.stringify(next) !== JSON.stringify(usage)) {
                usage = next;
                ops.push({ op: "usage", usage });
            }
            if (stopped || id !== conversation.id) return;
            const numbered = number === undefined ? {} : { number };
            if (ops.length > 0) send({ type: "turn", chat, ...numbered, ops });
            if (ops.some((op) => op.op === "reset"))
                send({ ...transcriptOf(chat, fold, usage), ...numbered });
        });
    };
    const agent = await addressed(home.harness, chat.with, context);
    if (agent === undefined)
        throw new Error(`no agent named \`${chat.with.agent}\``);
    const pointer = await home.harness.watchDoc(
        ChatDoc,
        agentKey(agent),
        context,
    );
    try {
        await bind();
        pointer?.start(bind);
    } catch (error) {
        await pointer?.stop();
        await watch?.stop();
        throw error;
    }
    return {
        stop: async () => {
            stopped = true;
            await pointer?.stop();
            await watch?.stop();
        },
    };
}

function transcriptOf(
    chat: ChatRef,
    fold: Fold,
    usage: UsageView,
    before?: string,
): Transcript {
    const end =
        before === undefined
            ? fold.items.length
            : fold.items.findIndex((item) => item.id === before);
    if (end < 0) throw new Error(`There is no item \`${before}\` here.`);
    const start = Math.max(0, end - 40);
    return {
        type: "transcript",
        chat,
        items: fold.items.slice(start, end),
        earlier: start === 0 ? null : (fold.items[start]?.id ?? null),
        ...fold.status,
        usage,
    };
}

/** The conversation `chat` names: the current one of the chief's chat with its agent, or a past one by number. */
async function conversationOf(
    home: Home,
    chat: ChatRef,
    context: Context,
): Promise<Conversation> {
    const agent = await addressed(home.harness, chat.with, context);
    if (agent === undefined) {
        throw new Error(`no agent named \`${chat.with.agent}\``);
    }
    if (chat.past === undefined) {
        return chatOf(home, { agent, chief: home.chief }, context);
    }
    const key = agentKey(agent);
    const past = (await home.harness.snapshot(ChatDoc, key, context))?.chats[
        home.chief
    ]?.conversations.find((each) => each.number === chat.past);
    const conversation =
        past && (await home.harness.conversation(past.conversation, context));
    if (!conversation) {
        throw new Error(`There is no past session \`${chat.past}\` here.`);
    }
    return conversation;
}

/** The transcript of `conversation` as of `view`, from the whole of its history. */
async function readBack(
    home: Home,
    conversation: Conversation,
    view: ConversationView,
    context: Context,
): Promise<Fold> {
    const fold = new Fold(home);
    const last = view.entries.reduce<EntryId | undefined>(
        (latest, entry) =>
            latest === undefined || entry.id > latest ? entry.id : latest,
        undefined,
    );
    let cursor: Cursor | undefined;
    while (last !== undefined) {
        const page = await conversation.entries(
            { order: "ascending", maxEntryId: last },
            PAGE,
            cursor,
            context,
        );
        for (const entry of page.items) fold.entry(entry);
        if (page.next === undefined) break;
        cursor = page.next;
    }
    fold.advance(view);
    return fold;
}

/** A chat's items as a client holds them, kept in step with the conversation, with the ops that keep a client in step. */
class Fold {
    readonly items: Item[] = [];
    status = { working: false, pending: 0 };
    /** True once an answer landed without some of what it showed being written, which no op takes away. */
    stale = false;
    #ops: Op[] = [];
    #last: EntryId | undefined;
    /** Tool items by their call's id. */
    readonly #calls = new Map<string, Item>();
    /** Items of the answer being written, by content block. */
    readonly #live = new Map<number, Item>();

    readonly #home: Home;

    constructor(home: Home) {
        this.#home = home;
    }

    /** Takes in what `view` holds beyond what was folded, and gives the ops for it. */
    advance(view: ConversationView): Op[] {
        // A summary leads the active range ahead of older kept entries.
        const fresh = view.entries
            .filter(
                (entry) => this.#last === undefined || entry.id > this.#last,
            )
            .sort((a, b) => a.id - b.id);
        for (const entry of fresh) this.entry(entry);
        const live = view.docs["pi.live"] as LiveState | undefined;
        const message = live?.generation?.message as
            | AssistantMessage
            | undefined;
        if (message !== undefined) this.#partial(message);
        const inbox = view.docs["pi.inbox"] as InboxState | undefined;
        const status = {
            working: live?.run !== undefined,
            pending: (inbox?.items ?? []).filter(
                (item) => item.mode !== "write",
            ).length,
        };
        if (
            status.working !== this.status.working ||
            status.pending !== this.status.pending
        ) {
            this.status = status;
            this.#ops.push({ op: "status", ...status, last: null });
        }
        return this.#ops.splice(0);
    }

    /** Takes in one entry of the conversation. */
    entry(entry: EntryRecord): void {
        this.#last = entry.id;
        const message = entry.model?.[0];
        if (ToolResultEntry.is(entry) && message?.role === "toolResult") {
            const item = this.#calls.get(message.toolCallId);
            if (item?.tool === undefined) return;
            const end = ending(message);
            Object.assign(item.tool, end);
            this.#ops.push({ op: "close", item: item.id, ...end });
            return;
        }
        const answer = AssistantEntry.is(entry);
        for (const [block, item] of itemsOf(entry)) {
            const live = answer ? this.#live.get(block) : undefined;
            if (answer) this.#live.delete(block);
            this.#add(item, live);
            if (item.tool !== undefined && message?.role === "assistant") {
                const call = message.content[block];
                if (call?.type === "toolCall") this.#calls.set(call.id, item);
            }
        }
        if (answer) {
            this.stale ||= this.#live.size > 0;
            this.#live.clear();
            return;
        }
        // The answer being written stays last, as it is read back.
        for (const live of this.#live.values()) this.#add(live, live);
    }

    /** Takes in the answer being written. */
    #partial(message: AssistantMessage): void {
        for (const [block, item] of blocksOf(message, (i) => `live:${i}`)) {
            if (item.tool !== undefined) continue;
            // A partial credential's prefix stays held until the next committed chunk.
            item.text = new Scrubber(this.#home.credentials.secrets()).push(
                item.text,
            );
            const shown = this.#live.get(block);
            if (shown === undefined) {
                this.#live.set(block, item);
                this.#add(item, undefined);
            } else if (
                item.text.length > shown.text.length &&
                item.text.startsWith(shown.text)
            ) {
                const text = item.text.slice(shown.text.length);
                shown.text = item.text;
                this.#ops.push({ op: "append", item: shown.id, text });
            }
        }
    }

    /** Adds `item` last, in the place of `replaced` when it is given. */
    #add(item: Item, replaced: Item | undefined): void {
        if (replaced !== undefined) {
            this.items.splice(this.items.indexOf(replaced), 1);
        }
        this.items.push(item);
        this.#ops.push({
            op: "add",
            item: structuredClone(item),
            ...(replaced === undefined ? {} : { replaces: replaced.id }),
        });
    }
}

/** The items `entry` shows, by the content block each comes from. */
function itemsOf(entry: EntryRecord): [number, Item][] {
    const id = `${entry.id}:0`;
    if (ChiefEntry.is(entry)) {
        const { text, chief, at } = entry.data;
        return [[0, { id, at, kind: "user", text, chief, rewindable: true }]];
    }
    if (NoticeEntry.is(entry)) {
        const { text, at } = entry.data;
        return [[0, { id, at, kind: "notice", text }]];
    }
    if (CompactionEntry.is(entry)) {
        return [
            [
                0,
                {
                    id,
                    at: entry.model?.[0]?.timestamp ?? 0,
                    kind: "notice",
                    text: SUMMARY_NOTICE,
                },
            ],
        ];
    }
    if (ReflectionEntry.is(entry)) {
        const { at } = entry.data;
        return [[0, { id, at, kind: "notice", text: "reflection" }]];
    }
    const message = entry.model?.[0];
    if (RetryEntry.is(entry)) {
        const { at } = entry.data;
        return [[0, { id, at, kind: "notice", text: textOf(message) }]];
    }
    if (UserEntry.is(entry) && message?.role === "user") {
        const at = message.timestamp;
        return [[0, { id, at, kind: "user", text: textOf(message) }]];
    }
    if (AssistantEntry.is(entry) && message?.role === "assistant") {
        return blocksOf(message, (block) => `${entry.id}:${block}`);
    }
    return [];
}

/** The items of an answer's content blocks, and the error it ended with. */
function blocksOf(
    message: AssistantMessage,
    id: (block: number) => string,
): [number, Item][] {
    const at = message.timestamp;
    const items = message.content.flatMap((block, index): [number, Item][] => {
        if (block.type === "toolCall") {
            const summary = [...JSON.stringify(block.arguments)]
                .slice(0, SUMMARY)
                .join("");
            const tool = {
                name: block.name,
                summary,
                exit: null,
                ms: null,
                failure: null,
                change:
                    block.name === "write"
                        ? null
                        : (changeOf(block.name, block.arguments) ?? null),
            };
            return [
                [index, { id: id(index), at, kind: "tool", text: "", tool }],
            ];
        }
        const [kind, text] =
            block.type === "text"
                ? (["assistant", block.text] as const)
                : (["reasoning", block.thinking] as const);
        return text.trim() === ""
            ? []
            : [[index, { id: id(index), at, kind, text }]];
    });
    if (message.stopReason === "error" || message.stopReason === "aborted") {
        const block = message.content.length;
        const text = message.errorMessage ?? message.stopReason;
        items.push([block, { id: id(block), at, kind: "error", text }]);
    }
    return items;
}

/** The end of a tool call, from its result. */
function ending(message: Extract<Message, { role: "toolResult" }>): {
    exit: number;
    ms: number | null;
    failure: string | null;
    change?: import("./protocol.ts").Change;
} {
    const code = (message.details as { exitCode?: unknown } | null | undefined)
        ?.exitCode;
    const failure = message.isError
        ? (textOf(message)
              .replace(/<\/?harness>\n?/g, "")
              .replace(/^\[error\] /gm, "")
              .split("\n")
              .find((line) => line.trim() !== "")
              ?.replace(/^Error: /, "") ?? "")
        : null;
    return {
        exit: typeof code === "number" ? code : message.isError ? 1 : 0,
        ms: message.durationMs ?? null,
        failure,
        ...(record(message.details).change === undefined
            ? {}
            : {
                  change: record(message.details)
                      .change as import("./protocol.ts").Change,
              }),
    };
}

/** The text of a message's content. */
function textOf(message: Message | undefined): string {
    if (message === undefined || message.role === "assistant") return "";
    const { content } = message;
    if (typeof content === "string") return content;
    return content
        .flatMap((part) => (part.type === "text" ? [part.text] : []))
        .join("\n");
}

/** Kept edit previews, including the old content of a whole-file write (C30). */
export function turnTools() {
    return defineExtension({
        name: "hamlet.turns",
        hooks: [
            hook(ToolTask, {
                afterTool: async (_call, result, api, context) => {
                    const change = await api.memo<
                        import("./protocol.ts").Change
                    >(CHANGE, context);
                    return change === undefined
                        ? undefined
                        : {
                              ...result,
                              details: { ...record(result.details), change },
                          };
                },
            }),
        ],
    });
}
