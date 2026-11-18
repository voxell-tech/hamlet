// The limits a provider keeps on a plan or key: read when the server asks, and seen as turns run.
import type { Limits, LimitWindow } from "./protocol.ts";

/** Takes what a turn learnt of `provider`'s limits. */
type Listener = (provider: string, limits: Limits) => void;

const listeners = new Set<Listener>();

/** Calls `listener` with what each turn learns of a provider's limits, until the returned function is called. */
export function onSeen(listener: Listener): () => void {
    listeners.add(listener);
    return () => listeners.delete(listener);
}

/** Tells the listeners what a turn learnt of `provider`'s limits. */
export function seen(provider: string, limits: Limits): void {
    for (const listener of listeners) listener(provider, limits);
}

/** The code OpenAI refuses a Sign in with ChatGPT request with when a limit of the plan, or of Hamlet's share, is reached. */
export const CHATGPT_REFUSAL = "subscription_sharing_usage_limit_exceeded";

/**
 * What a ChatGPT refusal tells: that the plan refused. OpenAI says neither which limit nor when it lifts, and asks
 * apps not to infer either from the code.
 */
export function chatgptRefused(now = Date.now()): Limits {
    return { checked: now, state: "refused", windows: [] };
}

/** The limits of a provider that tells nothing of them. */
export function unknown(now = Date.now()): Limits {
    return { checked: now, state: "unknown", windows: [] };
}

/** Claude's windows of time: their names and lengths in minutes. */
const CLAUDE_WINDOWS: Record<string, { name: string; length: number }> = {
    five_hour: { name: "Session", length: 300 },
    seven_day: { name: "Weekly", length: 10_080 },
    seven_day_opus: { name: "Weekly · Opus", length: 10_080 },
    seven_day_sonnet: { name: "Weekly · Sonnet", length: 10_080 },
};

/** A Claude window as Claude's `/usage` gives it: use in percent and an ISO reset. */
type ClaudeWindow = {
    utilization: number | null;
    resets_at: string | null;
} | null;

/** The part of the Agent SDK's usage answer the limits are read from. */
export type ClaudeUsage = {
    subscription_type: string | null;
    rate_limits_available: boolean;
    rate_limits: {
        five_hour?: ClaudeWindow;
        seven_day?: ClaudeWindow;
        seven_day_opus?: ClaudeWindow;
        seven_day_sonnet?: ClaudeWindow;
        model_scoped?: {
            display_name: string;
            utilization: number | null;
            resets_at: string | null;
        }[];
    } | null;
};

/** The Claude plan's limits from the Agent SDK's usage answer. */
export function claudeUsage(usage: ClaudeUsage, now = Date.now()): Limits {
    const plan =
        usage.subscription_type === null || usage.subscription_type === ""
            ? undefined
            : usage.subscription_type.charAt(0).toUpperCase() +
              usage.subscription_type.slice(1);
    const limits = usage.rate_limits;
    if (!usage.rate_limits_available || limits === null)
        return { ...unknown(now), ...(plan === undefined ? {} : { plan }) };
    const windows: LimitWindow[] = [];
    const add = (id: string, window: ClaudeWindow | undefined) => {
        const known = CLAUDE_WINDOWS[id];
        if (window == null || known === undefined) return;
        windows.push(claudeWindow(id, known.name, known.length, window, now));
    };
    add("five_hour", limits.five_hour);
    add("seven_day", limits.seven_day);
    // The weekly windows of each model, in the newer list where it is there.
    if (limits.model_scoped === undefined) {
        add("seven_day_opus", limits.seven_day_opus);
        add("seven_day_sonnet", limits.seven_day_sonnet);
    } else
        for (const model of limits.model_scoped)
            windows.push(
                claudeWindow(
                    `model:${model.display_name}`,
                    `Weekly · ${model.display_name}`,
                    10_080,
                    model,
                    now,
                ),
            );
    return {
        checked: now,
        ...(plan === undefined ? {} : { plan }),
        state: windows.some((w) => (w.used ?? 0) >= 100) ? "refused" : "ok",
        windows,
    };
}

function claudeWindow(
    id: string,
    name: string,
    length: number,
    window: NonNullable<ClaudeWindow>,
    now: number,
): LimitWindow {
    const resets =
        window.resets_at === null ? Number.NaN : Date.parse(window.resets_at);
    return {
        id,
        name,
        length,
        ...(window.utilization === null
            ? {}
            : { used: percent(window.utilization) }),
        ...(Number.isFinite(resets) && resets > now ? { resets } : {}),
    };
}

/** What the Claude program says of a limit as a turn runs, a `rate_limit_event`'s info. */
export type ClaudeRateLimit = {
    status: "allowed" | "allowed_warning" | "rejected";
    /** Seconds since the epoch. */
    resetsAt?: number;
    rateLimitType?: string;
    /** From 0 to 1. */
    utilization?: number;
};

/** What a turn learnt from a `rate_limit_event`: the state, and the window it names where it names one. */
export function claudeEvent(info: ClaudeRateLimit, now = Date.now()): Limits {
    const known =
        info.rateLimitType === undefined
            ? undefined
            : CLAUDE_WINDOWS[info.rateLimitType];
    const windows: LimitWindow[] =
        known === undefined || info.rateLimitType === undefined
            ? []
            : [
                  {
                      id: info.rateLimitType,
                      name: known.name,
                      length: known.length,
                      ...(info.utilization === undefined
                          ? info.status === "rejected"
                              ? { used: 100 }
                              : {}
                          : { used: percent(info.utilization * 100) }),
                      ...(info.resetsAt === undefined
                          ? {}
                          : { resets: info.resetsAt * 1000 }),
                  },
              ];
    return {
        checked: now,
        state:
            info.status === "rejected"
                ? "refused"
                : info.status === "allowed_warning"
                  ? "warning"
                  : "ok",
        windows,
    };
}

/** OpenRouter's key caps and how often each resets. */
const OPENROUTER_RESETS: Record<string, { name: string; length?: number }> = {
    daily: { name: "Daily cap", length: 1_440 },
    weekly: { name: "Weekly cap", length: 10_080 },
    monthly: { name: "Monthly cap" },
};

/**
 * An OpenRouter key's cap, from its documented `GET /key`. A key with no cap has no window and is never refused for
 * one.
 */
export async function openrouterLimits(
    key: string,
    baseUrl: string,
    signal: AbortSignal,
    now = Date.now(),
): Promise<Limits> {
    const response = await fetch(`${baseUrl.replace(/\/+$/, "")}/key`, {
        headers: { Authorization: `Bearer ${key}` },
        signal,
    });
    if (!response.ok)
        throw new Error(
            `HTTP ${response.status}: ${(await response.text()).slice(0, 200)}`,
        );
    const body = (await response.json()) as {
        data?: {
            limit?: number | null;
            limit_remaining?: number | null;
            limit_reset?: string | null;
        };
    };
    const data = body.data ?? {};
    const cap = data.limit;
    if (typeof cap !== "number" || !(cap > 0))
        return { checked: now, state: "ok", windows: [] };
    const left = Math.max(0, Math.min(cap, data.limit_remaining ?? cap));
    const reset =
        data.limit_reset == null
            ? undefined
            : OPENROUTER_RESETS[data.limit_reset];
    return {
        checked: now,
        state: left <= 0 ? "refused" : "ok",
        windows: [
            {
                id: "key",
                name: reset?.name ?? "Key cap",
                ...(reset?.length === undefined
                    ? {}
                    : { length: reset.length }),
                money: { cap, left },
                used: percent(((cap - left) / cap) * 100),
            },
        ],
    };
}

/** `value` as a percent the protocol takes: from 0 to 100, to a tenth. */
function percent(value: number): number {
    return Math.round(Math.max(0, Math.min(100, value)) * 10) / 10;
}
