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
import { claudeProvider } from "../src/claude.ts";

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

/** The provider, on the fake Claude program running `runs`, one list of steps per process. */
function provider(runs: unknown[][]) {
    const script = join(home, "script.json");
    writeFileSync(script, JSON.stringify({ runs, log }));
    vi.stubEnv("FAKE_CLAUDE_SCRIPT", script);
    const made = claudeProvider(home);
    if (made === undefined) throw new Error("no Claude program");
    const [model] = made.getModels();
    if (model === undefined) throw new Error("no Claude model");
    const call = (messages: Message[]) =>
        made
            .streamSimple(
                model,
                normalizeContext({
                    systemPrompt: "Be brief.",
                    messages,
                    tools: TOOLS,
                }),
                { sessionId: "conversation", apiKey: "marker" },
            )
            .result();
    return { call };
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

test("a tool call waits in its query for pi's result, and the next turn resumes pi's history", async () => {
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
    const resumed = noted()[2];
    expect(resumed).toMatchObject({
        run: 1,
        prompt: [{ type: "text", text: "Thanks" }],
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
    // The imported session is removed once its query ends.
    await vi.waitFor(() => expect(sessions()).toEqual([]));
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
