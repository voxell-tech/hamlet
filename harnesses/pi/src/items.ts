// Kept entries and answers as the link shows them.
import type { AssistantMessage, Message } from "@earendil-works/pi-ai";
import type { EntryRecord } from "@earendil-works/pi-durable";
import type { Block, Item, Mark } from "./link.ts";

/** Hamlet's kind and data of an entry, for one that is not pi's own. */
function markOf(entry: EntryRecord): Mark | undefined {
    if (entry.kind.startsWith("pi.")) return undefined;
    const data = entry.data;
    return {
        kind: entry.kind,
        data:
            typeof data === "object" && data !== null && !Array.isArray(data)
                ? { ...data }
                : {},
    };
}

/** The blocks of an answer, in its order. */
export function blocksOf(message: AssistantMessage): Block[] {
    return message.content.flatMap((part): Block[] => {
        switch (part.type) {
            case "text":
                return [{ type: "text", text: part.text }];
            case "thinking":
                return [{ type: "thinking", thinking: part.thinking }];
            case "toolCall":
                return [
                    {
                        type: "call",
                        id: part.id,
                        name: part.name,
                        arguments: { ...part.arguments },
                    },
                ];
            default:
                return [];
        }
    });
}

/** An answer as the link shows it, under the entry id `entry`. */
export function assistantItem(entry: string, message: AssistantMessage): Item {
    const { usage } = message;
    return {
        entry,
        kind: "assistant",
        at: message.timestamp,
        text: message.content
            .map((part) => (part.type === "text" ? part.text : ""))
            .join(""),
        blocks: blocksOf(message),
        stop: message.stopReason,
        ...(message.errorMessage === undefined
            ? {}
            : { failure: message.errorMessage }),
        usage: {
            input: usage.input,
            output: usage.output,
            cacheRead: usage.cacheRead,
            cacheWrite: usage.cacheWrite,
            cost: usage.cost.total,
        },
    };
}

/** A kept entry as the link shows it. */
export function itemOf(entry: EntryRecord, request?: string): Item {
    const id = String(entry.id);
    const mark = markOf(entry);
    const data = mark?.data;
    const marked = {
        ...(request === undefined ? {} : { request }),
        ...(mark === undefined ? {} : { mark }),
    };
    const messages: readonly Message[] = entry.model ?? [];
    const [message] = messages;
    const at =
        typeof data?.at === "number" && Number.isSafeInteger(data.at)
            ? Math.max(0, data.at)
            : (message?.timestamp ?? 0);
    if (message === undefined || messages.length > 1)
        return {
            entry: id,
            kind: entry.kind === "pi.compaction" ? "summary" : "other",
            at,
            ...marked,
            text: "",
        };
    switch (message.role) {
        case "user":
            return {
                entry: id,
                kind: "user",
                at,
                ...marked,
                text:
                    typeof message.content === "string"
                        ? message.content
                        : message.content
                              .map((part) =>
                                  part.type === "text" ? part.text : "",
                              )
                              .join("\n"),
            };
        case "assistant":
            return { ...assistantItem(id, message), ...marked };
        case "toolResult": {
            const details = message.details;
            return {
                entry: id,
                kind: "tool_result",
                at,
                ...marked,
                text: message.content
                    .map((part) => (part.type === "text" ? part.text : ""))
                    .join("\n"),
                call: message.toolCallId,
                tool: message.toolName,
                error: message.isError,
                ...(message.durationMs === undefined
                    ? {}
                    : { ms: Math.max(0, Math.round(message.durationMs)) }),
                ...(typeof details === "object" &&
                details !== null &&
                !Array.isArray(details)
                    ? { details: { ...details } }
                    : {}),
            };
        }
        case "system":
            return {
                entry: id,
                kind: entry.kind === "pi.compaction" ? "summary" : "other",
                at,
                ...marked,
                text: "",
            };
        default: {
            const unhandled: never = message;
            throw new Error(`Unhandled message ${String(unhandled)}`);
        }
    }
}
