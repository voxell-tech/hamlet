import { afterEach, expect, test, vi } from "vitest";
import { claudeEvent, claudeUsage, openrouterLimits } from "../src/limits.ts";

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
