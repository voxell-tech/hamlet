/** Entries as the chat items a client reads, folded as today folds them. */
import { isDeepStrictEqual } from "node:util";
import type {
    AssistantMessage,
    ToolResultMessage,
} from "@earendil-works/pi-ai";
import {
    type ConversationView,
    type EntryRecord,
    InboxDoc,
    type InboxState,
    LiveDoc,
    type LiveState,
    UsageDoc,
    type UsageState,
} from "@earendil-works/pi-durable";
import { clip, editOf, madeOf } from "./change.ts";
import { nameOf } from "./inputs.ts";
import type {
    ChatItem,
    ChatOp,
    ChatTool,
    Count,
    ItemKind,
    Usage,
} from "./messages/agent.ts";
import { u32 } from "./messages/shared.ts";
import { scrub, withheld } from "./models.ts";
import { type Kept, NOTE_BREAK } from "./turn.ts";

/** A call held for a chief, whose item stays open until its ruling. */
interface Held {
    readonly item: ChatItem;
    /** When the call began, in milliseconds. */
    readonly began: number;
}

interface Shown {
    readonly block: number;
    readonly item: ChatItem;
    readonly call?: string;
    /** The change a write shows once it ends having made its file. */
    readonly made?: ChatTool["change"];
}

/**
 * Turns a conversation's entries and the answer being written into ops on
 * its items, as `crates/hamlet_server/src/clients/fold.rs` folds them. It
 * keeps only the answer being written and the calls not yet answered, so a
 * watch holds no text for the whole wake.
 */
export class Fold {
    readonly #calls = new Map<string, Pick<Shown, "item" | "made">>();
    /** By the call the realm was sent. */
    readonly #held = new Map<string, Held>();
    readonly #live = new Map<number, ChatItem>();
    readonly #spent: Spent = { calls: 0, ms: 0 };
    #seen = -1;
    #lastInput = 0;
    working = false;
    pending = 0;
    usage: Usage = usageOf(undefined, 0, this.#spent);

    streamed(): ChatItem[] {
        return [...this.#live.values()];
    }

    view(view: ConversationView): ChatOp[] {
        const ops: ChatOp[] = [];
        for (const entry of view.entries) {
            if (Number(entry.id) > this.#seen) ops.push(...this.entry(entry));
        }
        const live = view.docs[LiveDoc.definition.kind] as
            | LiveState
            | undefined;
        ops.push(
            ...this.#partial(
                live?.generation?.message as AssistantMessage | undefined,
            ),
        );
        const inbox = view.docs[InboxDoc.definition.kind] as
            | InboxState
            | undefined;
        const working = live?.run !== undefined;
        const pending =
            inbox?.items.filter((item) => item.mode !== "write").length ?? 0;
        if (working !== this.working || pending !== this.pending) {
            this.working = working;
            this.pending = pending;
            ops.push({ type: "status", working, pending });
        }
        const usage = usageOf(
            view.docs[UsageDoc.definition.kind] as UsageState | undefined,
            this.#lastInput,
            this.#spent,
        );
        if (!isDeepStrictEqual(usage, this.usage)) {
            this.usage = usage;
            ops.push({ type: "usage", usage });
        }
        return ops;
    }

    entry(entry: EntryRecord): ChatOp[] {
        this.#seen = Math.max(this.#seen, Number(entry.id));
        this.#counted(entry);
        if (entry.kind === "pi.tool-result") return this.#ended(entry);
        const ruled = this.#ruled(entry);
        if (ruled !== undefined) return ruled;
        const answer = entry.kind === "pi.assistant";
        const ops = itemsOf(entry).map(({ block, item, call, made }) => {
            if (call !== undefined) this.#calls.set(call, { item, made });
            return added(item, answer ? this.#live.get(block)?.id : undefined);
        });
        if (answer) {
            this.#live.clear();
            const message = entry.model?.[0] as AssistantMessage | undefined;
            this.#lastInput = message?.usage.input ?? this.#lastInput;
        }
        return ops;
    }

    /** Counts a model call's answer into the chat's usage. */
    #counted(entry: EntryRecord): void {
        if (entry.kind !== "pi.assistant") return;
        const message = entry.model?.[0] as AssistantMessage | undefined;
        this.#spent.calls += 1;
        this.#spent.ms += message?.durationMs ?? 0;
    }

    #ended(entry: EntryRecord): ChatOp[] {
        const result = entry.model?.[0] as ToolResultMessage | undefined;
        const call =
            result === undefined
                ? undefined
                : this.#calls.get(result.toolCallId);
        const item = call?.item;
        if (result === undefined || item?.tool === undefined) return [];
        this.#calls.delete(result.toolCallId);
        const details = result.details as
            | { created?: unknown; waiting?: unknown }
            | undefined;
        if (typeof details?.waiting === "string") {
            this.#held.set(details.waiting, {
                item,
                began: result.timestamp - (result.durationMs ?? 0),
            });
            return [];
        }
        const made = details?.created === true ? call?.made : undefined;
        const text = result.content
            .map((part) => (part.type === "text" ? part.text : ""))
            .join("");
        return [
            closed(item, {
                exit: result.isError ? 1 : 0,
                ...(result.durationMs === undefined
                    ? {}
                    : { ms: u32(result.durationMs) }),
                ...(result.isError ? { failure: failureOf(text) } : {}),
                ...(made === undefined ? {} : { change: made }),
            }),
        ];
    }

    /**
     * Closes the item of the call a ruling's notice ends, as
     * `crates/hamlet_server/src/clients/fold.rs` closes a held call's item
     * when its result comes; none when no such item is open.
     */
    #ruled(entry: EntryRecord): ChatOp[] | undefined {
        if (entry.kind !== "hamlet.notice") return undefined;
        const { held } = (entry.data ?? {}) as Kept;
        const call = held === undefined ? undefined : this.#held.get(held.call);
        if (held === undefined || call === undefined) return undefined;
        this.#held.delete(held.call);
        const text = textOf(entry);
        const at = timeOf(entry);
        const ops = [
            closed(call.item, {
                exit: held.refused ? 1 : 0,
                ms: u32(at - call.began),
                ...(held.refused ? { failure: failureOf(text) } : {}),
            }),
        ];
        const said =
            call.item.tool?.name === "ask_chief" ? chiefSaid(text) : undefined;
        if (said !== undefined) {
            ops.push(
                added({
                    id: `${entry.id}:0`,
                    at: `${at}`,
                    kind: "answered",
                    text: said,
                }),
            );
        }
        return ops;
    }

    #partial(message: AssistantMessage | undefined): ChatOp[] {
        if (message === undefined) return [];
        const ops: ChatOp[] = [];
        const at = `${message.timestamp}`;
        for (const { block, item: whole } of blocksOf(
            message.content,
            (block) => `live:${block}`,
            at,
        )) {
            if (whole.tool !== undefined) continue;
            const scrubbed = scrub(whole);
            // A secret may yet be streaming in.
            const item = { ...scrubbed, text: withheld(scrubbed.text) };
            const shown = this.#live.get(block);
            if (shown !== undefined && item.text === shown.text) continue;
            this.#live.set(block, item);
            ops.push(
                shown !== undefined && item.text.startsWith(shown.text)
                    ? {
                          type: "append",
                          item: item.id,
                          at: Buffer.byteLength(shown.text),
                          text: item.text.slice(shown.text.length),
                      }
                    : { type: "add", item },
            );
        }
        return ops;
    }
}

/**
 * Every item a client reads goes through here or `#partial`, which scrub it,
 * so an `append`'s offset counts the text a client holds.
 */
function added(item: ChatItem, replaces?: string): ChatOp {
    return {
        type: "add",
        item: scrub(item),
        ...(replaces === undefined ? {} : { replaces }),
    };
}

/** `item`, a tool call's, as it ended. */
function closed(
    item: ChatItem,
    ended: Pick<ChatTool, "exit" | "ms" | "failure" | "change">,
): ChatOp {
    return added({
        ...item,
        tool: { ...(item.tool as ChatTool), ...ended },
    });
}

/** The words a chat shows where earlier entries were summarised (`clients/fold.rs`). */
const SUMMARY_NOTICE =
    "The conversation grew longer than the model can hold, so the earlier part of it was summarised.";

/** The items an entry shows; a tool's result shows in its call's item. */
function itemsOf(entry: EntryRecord): Shown[] {
    const at = `${timeOf(entry)}`;
    const one = (
        kind: ItemKind,
        text: string,
        more: Partial<ChatItem> = {},
    ): Shown[] => [
        { block: 0, item: { id: `${entry.id}:0`, at, kind, text, ...more } },
    ];
    switch (entry.kind) {
        case "pi.assistant": {
            const message = entry.model?.[0] as AssistantMessage | undefined;
            if (message === undefined) return [];
            const id = (block: number) => `${entry.id}:${block}`;
            const shown = blocksOf(message.content, id, at);
            if (
                message.stopReason === "error" ||
                message.stopReason === "aborted"
            ) {
                const block = message.content.length;
                shown.push({
                    block,
                    item: {
                        id: id(block),
                        at,
                        kind: "error",
                        text: message.errorMessage ?? message.stopReason,
                    },
                });
            }
            return shown;
        }
        case "pi.compaction":
            return one("notice", SUMMARY_NOTICE);
        // A notice the realm told the model is an input; one for people
        // alone keeps its text in its data. The chief's answer to an
        // `ask_chief` shows as `crates/hamlet_server/src/clients/fold.rs`
        // shows it.
        case "hamlet.notice": {
            const text =
                (entry.data as { text?: string } | undefined)?.text ??
                textOf(entry);
            const said = chiefSaid(text);
            return said === undefined
                ? one("notice", text)
                : one("answered", said);
        }
        case "hamlet.reflection":
            return one("notice", "reflection");
        case "hamlet.retry":
            return one("notice", textOf(entry));
        // The ask-again of a short cut-off (C28) shows as its summary's notice.
        case "hamlet.cut-off":
            return [];
        case "hamlet.line": {
            const { note, chief, images, files } = (entry.data ?? {}) as Kept;
            const line = one("user", textOf(entry), {
                rewindable: true,
                ...(chief === undefined ? {} : { chief }),
                ...(images === undefined ? {} : { images }),
                ...(files === undefined ? {} : { files }),
            });
            return note === undefined
                ? line
                : [
                      ...line,
                      {
                          block: 1,
                          item: {
                              id: `${entry.id}:1`,
                              at,
                              kind: "notice",
                              text: note,
                          },
                      },
                  ];
        }
        case "hamlet.worker-answer": {
            const { from } = (entry.data ?? {}) as Kept;
            return one(
                "user",
                textOf(entry),
                from === undefined ? {} : { author: nameOf(from, "worker") },
            );
        }
        case "hamlet.letter":
        case "hamlet.question":
        case "hamlet.answer": {
            const { from } = (entry.data ?? {}) as Kept;
            return one(
                "user",
                textOf(entry),
                from === undefined ? {} : { author: nameOf(from, "hire") },
            );
        }
        default:
            return entry.kind === "pi.user" || entry.kind.startsWith("hamlet.")
                ? one("user", textOf(entry))
                : [];
    }
}

function blocksOf(
    content: AssistantMessage["content"],
    id: (block: number) => string,
    at: Count,
): Shown[] {
    const shown: Shown[] = [];
    for (const [block, part] of content.entries()) {
        const item = { id: id(block), at };
        if (part.type === "toolCall") {
            const made =
                part.name === "write" ? madeOf(part.arguments) : undefined;
            shown.push({
                block,
                call: part.id,
                ...(made === undefined ? {} : { made }),
                item: {
                    ...item,
                    kind: "tool",
                    text: "",
                    tool: toolOf(part.name, part.arguments),
                },
            });
        } else if (part.type === "text" && part.text.trim() !== "") {
            shown.push({
                block,
                item: { ...item, kind: "assistant", text: part.text },
            });
        } else if (part.type === "thinking" && part.thinking.trim() !== "") {
            shown.push({
                block,
                item: { ...item, kind: "reasoning", text: part.thinking },
            });
        }
    }
    return shown;
}

/** `crates/hamlet_server/src/talk/asking.rs`'s `ANSWERS`. */
const ANSWERS = "The chief answers:";

/** The chief's words in the realm's answer to an `ask_chief`, as `talk/asking.rs`'s `said` reads them. */
function chiefSaid(result: string): string | undefined {
    if (!result.startsWith(ANSWERS)) return undefined;
    const words = result.slice(ANSWERS.length).trim();
    return words === "" ? undefined : words;
}

/** In milliseconds. */
export function timeOf(entry: EntryRecord): number {
    const message = entry.model?.[0];
    if (message !== undefined) return message.timestamp;
    const at = (entry.data as { at?: unknown } | undefined)?.at;
    return typeof at === "number" ? at : 0;
}

/** Characters of a tool's arguments an item shows (`clients/fold.rs`). */
const SUMMARY = 200;

function toolOf(name: string, args: unknown): NonNullable<ChatItem["tool"]> {
    const change = name === "edit" ? editOf(args) : undefined;
    return {
        name,
        summary: clip(
            JSON.stringify(args, (_key, value: unknown) =>
                value !== null &&
                typeof value === "object" &&
                !Array.isArray(value)
                    ? Object.fromEntries(
                          Object.entries(value).sort(([a], [b]) =>
                              a < b ? -1 : a > b ? 1 : 0,
                          ),
                      )
                    : value,
            ) ?? "null",
            SUMMARY,
        ),
        ...(change === undefined ? {} : { change }),
    };
}

/** The first line of a failed call's text that says something (`clients/fold.rs`). */
function failureOf(text: string): string {
    const line =
        text
            .replaceAll("<harness>\n", "")
            .replaceAll("</harness>\n", "")
            .replaceAll("<harness>", "")
            .replaceAll("</harness>", "")
            .split("\n")
            .map((part) => (part.startsWith("[error] ") ? part.slice(8) : part))
            .find((part) => part.trim() !== "") ?? "";
    return line.startsWith("Error: ") ? line.slice(7) : line;
}

/** The model calls a chat's answers made, and the time they took. */
interface Spent {
    calls: number;
    ms: number;
}

/** The conversation's spend, from pi-durable's ledger and its answers. */
function usageOf(
    state: UsageState | undefined,
    lastInput: number,
    spent: Spent,
): Usage {
    let input = 0;
    let cached = 0;
    let output = 0;
    let reasoning = 0;
    let dollars = 0;
    for (const used of Object.values(state?.models ?? {})) {
        input += used.input;
        cached += used.cacheRead;
        output += used.output;
        reasoning += used.reasoning ?? 0;
        dollars += used.cost.total;
    }
    const micros = Math.round(dollars * 1_000_000);
    return {
        calls: `${spent.calls}`,
        unreported: "0",
        input: `${input}`,
        cached: `${cached}`,
        output: `${output}`,
        reasoning: `${reasoning}`,
        ms: `${spent.ms}`,
        micros: `${micros}`,
        last_input: `${lastInput}`,
        summary: `${input} in, ${output} out, ${money(micros)}`,
    };
}

/** As `crates/hamlet_server/src/spend.rs` shows money. */
export function money(micros: number): string {
    if (micros >= 1 && micros < 10_000) return "<$0.01";
    const cents = Math.floor((micros + 5_000) / 10_000);
    return `$${Math.floor(cents / 100)}.${`${cents % 100}`.padStart(2, "0")}`;
}

/** An entry's words; a reviewed line's are the chief's own, without the lead's note. */
export function textOf(entry: EntryRecord): string {
    const text = (entry.model ?? [])
        .flatMap((message) =>
            typeof message.content === "string"
                ? [message.content]
                : message.content.flatMap((part) =>
                      part.type === "text" ? [part.text] : [],
                  ),
        )
        .join("");
    const { note } = (entry.data ?? {}) as Kept;
    const noted = note === undefined ? undefined : `${NOTE_BREAK}${note}`;
    return noted !== undefined && text.endsWith(noted)
        ? text.slice(0, -noted.length)
        : text;
}
