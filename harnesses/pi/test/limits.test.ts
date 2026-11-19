import { afterEach, expect, test, vi } from "vitest";
import {
    CHATGPT_USAGE,
    chatgptLimits,
    chatgptUsage,
    claudeEvent,
    claudeUsage,
    openrouterLimits,
} from "../src/limits.ts";

afterEach(() => {
    vi.unstubAllGlobals();
});

const NOW = Date.parse("2100-01-01T12:00:00Z");

test("a Claude login with no plan limits tells its plan and nothing else", () => {
    expect(
        claudeUsage(
            {
                subscription_type: "team",
                rate_limits_available: false,
                rate_limits: null,
            },
            NOW,
        ),
    ).toEqual({ checked: NOW, plan: "Team", state: "unknown", windows: [] });
});

test("Claude's model windows come from the older keys when the newer list is not there", () => {
    const read = claudeUsage(
        {
            subscription_type: null,
            rate_limits_available: true,
            rate_limits: {
                five_hour: {
                    utilization: 100,
                    resets_at: "2100-01-01T13:00:00Z",
                },
                seven_day_opus: { utilization: 12.34, resets_at: null },
                seven_day_sonnet: null,
            },
        },
        NOW,
    );
    expect(read.state).toBe("refused");
    expect(read.plan).toBeUndefined();
    expect(read.windows).toEqual([
        {
            id: "five_hour",
            name: "Session",
            length: 300,
            used: 100,
            resets: Date.parse("2100-01-01T13:00:00Z"),
        },
        {
            id: "seven_day_opus",
            name: "Weekly · Opus",
            length: 10_080,
            used: 12.3,
        },
    ]);
});

test("a refusal Claude names no window for still refuses", () => {
    expect(claudeEvent({ status: "rejected" }, NOW)).toEqual({
        checked: NOW,
        state: "refused",
        windows: [],
    });
    expect(
        claudeEvent({ status: "rejected", rateLimitType: "seven_day" }, NOW)
            .windows,
    ).toEqual([{ id: "seven_day", name: "Weekly", length: 10_080, used: 100 }]);
});

/** `fetch` answering OpenRouter's `GET /key` with `data`, and the requests it was given. */
function openrouter(data: unknown, status = 200) {
    const asked: Request[] = [];
    vi.stubGlobal(
        "fetch",
        vi.fn(async (url: string, init: RequestInit) => {
            asked.push(new Request(url, init));
            return new Response(JSON.stringify({ data }), { status });
        }),
    );
    return asked;
}

test("an OpenRouter key's cap is read with the key, as money left", async () => {
    const asked = openrouter({
        limit: 100,
        limit_remaining: 74.5,
        limit_reset: "monthly",
    });
    const read = await openrouterLimits(
        "sk-or-key",
        "https://openrouter.ai/api/v1/",
        AbortSignal.timeout(1_000),
        NOW,
    );
    expect(asked.map((r) => r.url)).toEqual([
        "https://openrouter.ai/api/v1/key",
    ]);
    expect(asked[0]?.headers.get("authorization")).toBe("Bearer sk-or-key");
    expect(read).toEqual({
        checked: NOW,
        state: "ok",
        windows: [
            {
                id: "key",
                name: "Monthly cap",
                money: { cap: 100, left: 74.5 },
                used: 25.5,
            },
        ],
    });
});

test("an OpenRouter key with no cap is never refused for one, and a spent cap is", async () => {
    openrouter({ limit: null, limit_remaining: null, limit_reset: null });
    expect(
        await openrouterLimits(
            "k",
            "https://o",
            AbortSignal.timeout(1_000),
            NOW,
        ),
    ).toEqual({ checked: NOW, state: "ok", windows: [] });
    openrouter({ limit: 5, limit_remaining: 0, limit_reset: "daily" });
    const spent = await openrouterLimits(
        "k",
        "https://o",
        AbortSignal.timeout(1_000),
        NOW,
    );
    expect(spent.state).toBe("refused");
    expect(spent.windows[0]).toMatchObject({
        name: "Daily cap",
        length: 1_440,
        used: 100,
    });
});

test("OpenRouter refusing the key is an error, not limits", async () => {
    openrouter({ error: "no" }, 401);
    await expect(
        openrouterLimits("k", "https://o", AbortSignal.timeout(1_000), NOW),
    ).rejects.toThrow(/HTTP 401/);
});

/** An access token as ChatGPT's sign-in gives one, for the account `account`. */
function token(account: string): string {
    const part = (value: unknown) =>
        Buffer.from(JSON.stringify(value)).toString("base64url");
    return `${part({ alg: "none" })}.${part({
        "https://api.openai.com/auth": { chatgpt_account_id: account },
    })}.signature`;
}

test("a ChatGPT plan's use is read with its token and account, as Codex reads it", async () => {
    const asked: Request[] = [];
    vi.stubGlobal(
        "fetch",
        vi.fn(async (url: string, init: RequestInit) => {
            asked.push(new Request(url, init));
            return new Response(
                JSON.stringify({
                    plan_type: "plus",
                    rate_limit: {
                        allowed: true,
                        limit_reached: false,
                        primary_window: {
                            used_percent: 42,
                            limit_window_seconds: 18_000,
                            reset_after_seconds: 3_600,
                            reset_at: NOW / 1000 + 3_600,
                        },
                        secondary_window: {
                            used_percent: 7.25,
                            limit_window_seconds: 604_800,
                            reset_at: NOW / 1000 + 86_400,
                        },
                    },
                }),
            );
        }),
    );
    const read = await chatgptLimits(
        token("acct-1"),
        AbortSignal.timeout(1_000),
        NOW,
    );
    expect(asked[0]?.url).toBe(CHATGPT_USAGE);
    expect(asked[0]?.headers.get("authorization")).toBe(
        `Bearer ${token("acct-1")}`,
    );
    expect(asked[0]?.headers.get("chatgpt-account-id")).toBe("acct-1");
    expect(read).toEqual({
        checked: NOW,
        plan: "Plus",
        state: "ok",
        windows: [
            {
                id: "primary",
                name: "Session",
                length: 300,
                used: 42,
                resets: NOW + 3_600_000,
            },
            {
                id: "secondary",
                name: "Weekly",
                length: 10_080,
                used: 7.3,
                resets: NOW + 86_400_000,
            },
        ],
    });
});

test("a ChatGPT limit that is reached refuses, and a model's own limits are named for it", () => {
    const read = chatgptUsage(
        {
            plan_type: "pro",
            rate_limit: {
                limit_reached: true,
                primary_window: {
                    used_percent: 100,
                    limit_window_seconds: 18_000,
                    reset_after_seconds: 600,
                },
            },
            additional_rate_limits: [
                {
                    limit_name: "GPT-5-Codex-Spark",
                    metered_feature: "codex_spark",
                    rate_limit: {
                        secondary_window: {
                            used_percent: 3,
                            limit_window_seconds: 604_800,
                        },
                    },
                },
            ],
        },
        NOW,
    );
    expect(read.state).toBe("refused");
    expect(read.windows).toEqual([
        {
            id: "primary",
            name: "Session",
            length: 300,
            used: 100,
            resets: NOW + 600_000,
        },
        {
            id: "codex_spark:secondary",
            name: "Weekly · GPT-5-Codex-Spark",
            length: 10_080,
            used: 3,
        },
    ]);
});

test("ChatGPT refusing the read is an error, not limits", async () => {
    vi.stubGlobal(
        "fetch",
        vi.fn(async () => new Response("no", { status: 401 })),
    );
    await expect(
        chatgptLimits(token("a"), AbortSignal.timeout(1_000), NOW),
    ).rejects.toThrow("HTTP 401");
});
