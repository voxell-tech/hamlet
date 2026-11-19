import {
    calculateContextTokens,
    estimateMessageTokens,
} from "@earendil-works/pi-ai/utils/estimate";
import type { ContextView } from "@earendil-works/pi-durable";

/**
 * The tokens of the next request over `view`, counted as pi's compaction counts them: the usage of the newest answer
 * after the head, then estimates of the messages after it.
 */
export function contextTokens(view: ContextView): number {
    const after = view.head?.id ?? Number.NEGATIVE_INFINITY;
    let measured: (typeof view.messages)[number] | undefined;
    for (let index = view.entries.length - 1; index >= 0; index--) {
        // Entries kept from before the head were measured in a larger context.
        if ((view.entries[index]?.id ?? after) <= after) continue;
        measured = view.contributions[index]?.findLast(
            (message) =>
                message.role === "assistant" &&
                calculateContextTokens(message.usage) > 0,
        );
        if (measured !== undefined) break;
    }
    const from =
        measured === undefined ? 0 : view.messages.lastIndexOf(measured) + 1;
    let tokens =
        measured?.role === "assistant"
            ? calculateContextTokens(measured.usage)
            : 0;
    for (const message of view.messages.slice(from))
        tokens += estimateMessageTokens(message);
    return tokens;
}
