// The entries a conversation brought in from another program begins with.
import type { AssistantMessage, UserMessage } from "@earendil-works/pi-ai";
import type { EntryDraft, JsonObject } from "@earendil-works/pi-durable";
import type { Earlier } from "./protocol.ts";

/** The api an answer from another program names, which no provider serves. */
const IMPORTED_API = "imported";

/** `earlier` as entries: messages the model is given, and marks for people alone. */
export function entriesOf(earlier: readonly Earlier[]): EntryDraft[] {
    return earlier.map((entry): EntryDraft => {
        switch (entry.type) {
            case "user": {
                const message: UserMessage = {
                    role: "user",
                    content: entry.text,
                    timestamp: entry.at,
                };
                return {
                    kind: entry.mark.kind,
                    data: entry.mark.data as JsonObject,
                    model: [message],
                };
            }
            case "assistant": {
                const message: AssistantMessage = {
                    role: "assistant",
                    content: [{ type: "text", text: entry.text }],
                    api: IMPORTED_API,
                    provider: entry.source,
                    model: entry.model,
                    usage: {
                        input: 0,
                        output: 0,
                        cacheRead: 0,
                        cacheWrite: 0,
                        totalTokens: 0,
                        cost: {
                            input: 0,
                            output: 0,
                            cacheRead: 0,
                            cacheWrite: 0,
                            total: 0,
                        },
                    },
                    stopReason: "stop",
                    timestamp: entry.at,
                };
                return { kind: "pi.assistant", model: [message] };
            }
            case "note":
                return {
                    kind: entry.mark.kind,
                    data: entry.mark.data as JsonObject,
                };
            default: {
                const unhandled: never = entry;
                throw new Error(`Unhandled entry ${String(unhandled)}`);
            }
        }
    });
}
