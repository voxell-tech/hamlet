import { randomUUID } from "node:crypto";
import {
    existsSync,
    mkdtempSync,
    readdirSync,
    readFileSync,
    rmSync,
    writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { AssistantMessage, Message, Tool } from "@earendil-works/pi-ai";
import { normalizeContext } from "@earendil-works/pi-ai/utils/transcript";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { claudeProvider, programOf, signOut } from "../src/claude.ts";
import { limits } from "../src/claude-query.ts";
import { onSeen } from "../src/limits.ts";
import type { Limits } from "../src/protocol.ts";

const FAKE = join(import.meta.dirname, "fixtures", "claude.mjs");
const TOOLS: Tool[] = [
    {
        name: "read",
        description: "Read a file.",
        parameters: {
            type: "object",
            properties: { path: { type: "string" } },
        } as never,
    },
];

let home = "";
let log = "";

beforeEach(() => {
    home = mkdtempSync(join(tmpdir(), "hamlet-claude-"));
    log = join(home, "fake.jsonl");
    vi.stubEnv("HAMLET_CLAUDE_EXECUTABLE", FAKE);
    // The chief's own key, which the Claude program must not see.
    vi.stubEnv("ANTHROPIC_API_KEY", "chief-key");
});

afterEach(() => {
    vi.unstubAllEnvs();
    rmSync(home, { recursive: true, force: true });
});

/** The provider, on the fake Claude program running `runs`, one list of steps per process, and answering `usage`. */
function provider(runs: unknown[][], usage?: unknown) {
    const script = join(home, "script.json");
    writeFileSync(script, JSON.stringify({ runs, log, usage }));
    vi.stubEnv("FAKE_CLAUDE_SCRIPT", script);
    const made = claudeProvider(home);
    if (made === undefined) throw new Error("no Claude program");
    const [model] = made.getModels();
    if (model === undefined) throw new Error("no Claude model");
    // The provider keeps sessions by pi's, which outlive a test.
    const sessionId = randomUUID();
    const call = (messages: Message[]) =>
        made
            .streamSimple(
                model,
                normalizeContext({
                    systemPrompt: "Be brief.",
                    messages,
                    tools: TOOLS,
                }),
                { sessionId, apiKey: "marker" },
            )
            .result();
    return { call, made };
}

function noted(): Record<string, unknown>[] {
    return readFileSync(log, "utf8")
        .trim()
        .split("\n")
        .map((line) => JSON.parse(line));
}

const user = (text: string): Message => ({
    role: "user",
    content: text,
    timestamp: 0,
});

const say = (message: Record<string, unknown>) => ({ say: message });

/** Claude's answer `text`, streamed as message events, then the end of the query's turn. */
const streamed = (text: string) => [
    say({
        type: "stream_event",
        parent_tool_use_id: null,
        event: {
            type: "message_start",
            message: {
                id: "msg_2",
                usage: { input_tokens: 30, output_tokens: 1 },
            },
        },
    }),
    say({
        type: "stream_event",
        parent_tool_use_id: null,
        event: {
            type: "content_block_start",
            index: 0,
            content_block: { type: "text", text: "" },
        },
    }),
    say({
        type: "stream_event",
        parent_tool_use_id: null,
        event: {
            type: "content_block_delta",
            index: 0,
            delta: { type: "text_delta", text },
        },
    }),
    say({
        type: "stream_event",
        parent_tool_use_id: null,
        event: { type: "content_block_stop", index: 0 },
    }),
    say({
        type: "stream_event",
        parent_tool_use_id: null,
        event: {
            type: "message_delta",
            delta: { stop_reason: "end_turn" },
            usage: { output_tokens: 7 },
        },
    }),
    say({
        type: "stream_event",
        parent_tool_use_id: null,
        event: { type: "message_stop" },
    }),
    say({ type: "result", subtype: "success", is_error: false, result: text }),
];

/** Claude's answer `text`, delivered whole, then the end of the query's turn. */
const whole = (text: string) => [
    say({
        type: "assistant",
        parent_tool_use_id: null,
        message: {
            id: "msg_3",
            model: "claude-fake",
            content: [{ type: "text", text }],
            usage: { input_tokens: 40, output_tokens: 3 },
        },
    }),
    say({ type: "result", subtype: "success", is_error: false, result: text }),
];

test("a tool call waits in its query for pi's result, and the next input resumes the same Claude Code session", async () => {
    const { call } = provider([
        [
            say({
                type: "assistant",
                parent_tool_use_id: null,
                message: {
                    id: "msg_1",
                    model: "claude-fake",
                    content: [
                        {
                            type: "tool_use",
                            id: "toolu_1",
                            name: "mcp__hamlet__read",
                            input: { path: "a" },
                        },
                    ],
                    usage: { input_tokens: 20, output_tokens: 5 },
                },
            }),
            { call: { id: "toolu_1", name: "read", arguments: { path: "a" } } },
            ...streamed("It says hi."),
        ],
        whole("You're welcome."),
    ]);
    const asked = user("Read a");
    const calls = await call([asked]);
    expect(calls.stopReason).toBe("toolUse");
    expect(calls.content).toEqual([
        {
            type: "toolCall",
            id: "toolu_1",
            name: "read",
            arguments: { path: "a" },
        },
    ]);

    const result: Message = {
        role: "toolResult",
        toolCallId: "toolu_1",
        toolName: "read",
        content: [{ type: "text", text: "hi" }],
        isError: false,
        timestamp: 0,
    };
    const answered = await call([asked, calls, result]);
    expect(answered.stopReason).toBe("stop");
    expect(text(answered)).toBe("It says hi.");
    expect(answered.usage).toMatchObject({ input: 30, output: 7 });
    // One process answered both calls, the result reaching the call that waited for it.
    expect(noted()).toMatchObject([
        { run: 0, prompt: [{ type: "text", text: "Read a" }] },
        { run: 0, result: { content: [{ type: "text", text: "hi" }] } },
    ]);

    const thanks = await call([asked, calls, result, answered, user("Thanks")]);
    expect(text(thanks)).toBe("You're welcome.");
    // The next input resumes the first query's own session, given only what is new.
    const first = String(noted()[0]?.session);
    const resumed = noted()[2];
    expect(resumed).toMatchObject({
        run: 1,
        prompt: [{ type: "text", text: "Thanks" }],
        resume: first,
        config: join(home, "claude"),
        apiKey: null,
    });
    const records = String(resumed?.resumed);
    for (const seen of [
        "Read a",
        '"name":"mcp__hamlet__read"',
        '"tool_use_id":"toolu_1"',
        "It says hi.",
    ])
        expect(records).toContain(seen);
    // That session is kept for the input after, and no other is left.
    await vi.waitFor(() => expect(sessions()).toEqual([`${first}.jsonl`]));
});

test("an input after a change to the history before it starts from pi's history in a new session", async () => {
    const { call } = provider([whole("Hi."), whole("Again, hi.")]);
    const hello = await call([user("Hello")]);
    expect(text(hello)).toBe("Hi.");
    const first = String(noted()[0]?.session);

    // pi summarised or edited what came before, so the first session no longer holds it.
    const again = await call([user("Hello, edited"), hello, user("Again")]);
    expect(text(again)).toBe("Again, hi.");
    const second = noted()[1];
    expect(second).toMatchObject({
        run: 1,
        prompt: [{ type: "text", text: "Again" }],
    });
    expect(second?.resume).not.toBe(first);
    expect(String(second?.resumed)).toContain("Hello, edited");
    // The first session is removed, and the new one kept.
    await vi.waitFor(() =>
        expect(sessions()).toEqual([`${String(second?.session)}.jsonl`]),
    );
});

test("a turn Claude bills beyond the plan fails", async () => {
    const { call } = provider([
        [
            say({
                type: "rate_limit_event",
                rate_limit_info: { status: "rejected", isUsingOverage: true },
            }),
            ...whole("Billed."),
        ],
    ]);
    const answer = await call([user("Hello")]);
    expect(answer.stopReason).toBe("error");
    expect(answer.errorMessage).toMatch(/beyond your plan/);
});

test("a turn tells what Claude says of the plan's limits as it runs", async () => {
    const told: [string, Limits][] = [];
    const stop = onSeen((provider, limits) => told.push([provider, limits]));
    const { call } = provider([
        [
            say({
                type: "rate_limit_event",
                rate_limit_info: {
                    status: "allowed_warning",
                    rateLimitType: "five_hour",
                    utilization: 0.91,
                    resetsAt: 1_900_000_000,
                },
            }),
            ...whole("Close."),
        ],
    ]);
    try {
        await call([user("Hello")]);
    } finally {
        stop();
    }
    expect(told).toEqual([
        [
            "claude",
            {
                checked: expect.any(Number),
                state: "warning",
                windows: [
                    {
                        id: "five_hour",
                        name: "Session",
                        length: 300,
                        used: 91,
                        resets: 1_900_000_000_000,
                    },
                ],
            },
        ],
    ]);
});

test("the plan's limits are asked of the Claude program, without a turn", async () => {
    const { made } = provider([], {
        subscription_type: "max",
        rate_limits_available: true,
        rate_limits: {
            five_hour: { utilization: 46, resets_at: "2100-01-01T16:40:00Z" },
            seven_day: { utilization: 34, resets_at: "2100-01-05T09:10:00Z" },
            model_scoped: [
                {
                    display_name: "Fable",
                    utilization: 0,
                    resets_at: "2100-01-05T09:10:00Z",
                },
            ],
        },
        behaviors: null,
    });
    const program = programOf(made);
    if (program === undefined) throw new Error("no Claude program");
    const read = await limits(program, AbortSignal.timeout(10_000));
    expect(read).toEqual({
        checked: expect.any(Number),
        plan: "Max",
        state: "ok",
        windows: [
            {
                id: "five_hour",
                name: "Session",
                length: 300,
                used: 46,
                resets: Date.parse("2100-01-01T16:40:00Z"),
            },
            {
                id: "seven_day",
                name: "Weekly",
                length: 10_080,
                used: 34,
                resets: Date.parse("2100-01-05T09:10:00Z"),
            },
            {
                id: "model:Fable",
                name: "Weekly · Fable",
                length: 10_080,
                used: 0,
                resets: Date.parse("2100-01-05T09:10:00Z"),
            },
        ],
    });
    // No turn ran, so no prompt reached the program.
    expect(existsSync(log)).toBe(false);
});

test("signing out runs the Claude program's own sign-out in its folder", async () => {
    const { made } = provider([]);
    const program = programOf(made);
    if (program === undefined) throw new Error("no Claude program");
    await signOut(program, AbortSignal.timeout(10_000));
    expect(noted()).toEqual([{ logout: join(home, "claude") }]);
});

function text(message: AssistantMessage): string {
    return message.content
        .flatMap((block) => (block.type === "text" ? [block.text] : []))
        .join("");
}

function sessions(): string[] {
    const projects = join(home, "claude", "projects");
    return existsSync(projects)
        ? readdirSync(projects).flatMap((project) =>
              readdirSync(join(projects, project)),
          )
        : [];
}
