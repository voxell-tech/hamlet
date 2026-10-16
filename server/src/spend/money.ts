import type { AssistantMessage, Model, Usage } from "@earendil-works/pi-ai";
import { assumedRate } from "../models/endpoints.ts";

/** Spend in tokens, millionths of a dollar and elapsed milliseconds. */
export type Amount = { tokens: number; micros: number; ms: number };

/** An empty spend accumulator. */
export function zero(): Amount {
    return { tokens: 0, micros: 0, ms: 0 };
}

/** Adds spend in place, saturating each meter at the largest safe integer. */
export function add(to: Amount, amount: Readonly<Amount>): void {
    for (const key of ["tokens", "micros", "ms"] as const) {
        to[key] = Math.min(Number.MAX_SAFE_INTEGER, to[key] + amount[key]);
    }
}

/** Rounds nonnegative dollar cost to millionths of a dollar. */
export function micros(cost: number): number {
    return Math.round(Math.max(0, cost) * 1_000_000);
}

/** Formats millionths of a dollar, keeping positive sub-cent amounts visible. */
export function money(value: number): string {
    return value > 0 && value < 10_000
        ? "<$0.01"
        : `$${(value / 1_000_000).toFixed(2)}`;
}

/** Formats elapsed milliseconds in whole seconds, minutes or hours. */
export function duration(ms: number): string {
    const seconds = Math.floor(ms / 1000);
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
    return `${Math.floor(seconds / 3600)}h ${String(Math.floor((seconds % 3600) / 60)).padStart(2, "0")}m`;
}

/** The three meters of a response, priced when its provider reports no cost. */
export function amountOf(
    usage: Readonly<Usage>,
    ms: number,
    model: Model<string> | undefined,
    estimate = 0,
    billed?: number,
): Amount {
    const reported = usage.totalTokens > 0;
    const tokens = reported ? usage.totalTokens : estimate;
    const input = reported
        ? usage.input + usage.cacheRead + usage.cacheWrite
        : estimate;
    const fallback = assumedRate(model?.baseUrl ?? "");
    const known = model as
        | (Model<string> & { priceKnown?: boolean })
        | undefined;
    const rate =
        model?.cost &&
        (known?.priceKnown === true ||
            Object.values(model.cost).some(
                (n) => typeof n === "number" && n > 0,
            ))
            ? model.cost
            : { input: fallback, output: fallback };
    const priced = input * rate.input + usage.output * rate.output;
    return {
        tokens,
        micros:
            billed !== undefined
                ? micros(billed)
                : usage.cost.total > 0
                  ? micros(usage.cost.total)
                  : Math.round(priced),
        ms,
    };
}

/** Four characters per token when the provider reports none. */
export function estimateOf(
    messages: readonly unknown[],
    reply: AssistantMessage,
): number {
    return Math.floor(
        [...messages, reply].reduce<number>(
            (sum, value) => sum + Buffer.byteLength(JSON.stringify(value)),
            0,
        ) / 4,
    );
}
