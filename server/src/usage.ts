import { duration, money } from "./money.ts";
import type { UsageView } from "./protocol.ts";

/** The honest one-line account of a conversation's model calls (usage.rs:64-81). */
export function usageSummary(usage: Omit<UsageView, "summary">): string {
    if (usage.calls === 0) return "No model calls yet";
    const count = usage.input + usage.output;
    const tokens =
        count < 1000
            ? `${count} tok`
            : count < 1_000_000
              ? `${(count / 1000).toFixed(1)}k tok`
              : `${(count / 1_000_000).toFixed(2)}M tok`;
    const spent =
        usage.micros > 0
            ? money(usage.micros)
            : `${duration(usage.ms)} model time`;
    return `${usage.unreported === usage.calls ? "Tokens not reported" : tokens}, ${spent}`;
}
