import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
    existsSync,
    mkdirSync,
    mkdtempSync,
    readdirSync,
    readFileSync,
    rmSync,
    statSync,
    writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { type TestContext, test } from "node:test";
import { getSessionMessages } from "@anthropic-ai/claude-agent-sdk";
import type { MutableModels } from "@earendil-works/pi-ai";
import { chatActions } from "../src/agent/chat.ts";
import { withClaude } from "../src/agent/claude.ts";
import { signIns } from "../src/agent/login.ts";
import type { ChatPage, Kept, Login } from "../src/agent/messages.ts";
import {
    FAKE_ROOT,
    fakeCalls,
    fakeFiles,
    from,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";

const FAKE = join(import.meta.dirname, "fixtures", "claude-fake", "claude.mjs");

/** What the host's own environment holds, which the Claude program must not see. */
const HOST = {
    ANTHROPIC_API_KEY: "made-up-chief-key",
    CLAUDE_CODE_OAUTH_TOKEN: "made-up-chief-token",
    CLAUDE_CONFIG_DIR: "/made-up/chief/.claude",
    HAMLET_HOME: "/made-up/realm",
};

function planted(t: TestContext, env: Record<string, string>): void {
    const before = Object.fromEntries(
        Object.keys(env).map((name) => [name, process.env[name]]),
    );
    Object.assign(process.env, env);
    t.after(() => {
        for (const [name, value] of Object.entries(before)) {
            if (value === undefined) delete process.env[name];
            else process.env[name] = value;
        }
    });
}

const say = (message: Record<string, unknown>) => ({ say: message });

/** Claude's answer `text`, streamed as message events, then the end of the query's turn. */
const streamed = (text: string) => [
    ...[
        {
            type: "message_start",
            message: {
                id: "msg_3",
                usage: { input_tokens: 30, output_tokens: 1 },
            },
        },
        {
            type: "content_block_start",
            index: 0,
            content_block: { type: "text", text: "" },
        },
        {
            type: "content_block_delta",
            index: 0,
            delta: { type: "text_delta", text },
        },
        { type: "content_block_stop", index: 0 },
        {
            type: "message_delta",
            delta: { stop_reason: "end_turn" },
            usage: { output_tokens: 7 },
        },
        { type: "message_stop" },
    ].map((event) =>
        say({ type: "stream_event", parent_tool_use_id: null, event }),
    ),
    say({ type: "result", subtype: "success", is_error: false, result: text }),
];

/** Claude's call of the tool `read` on `path`, which the fake program then makes. */
const readCall = (path: string) => [
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
                    input: { path },
                },
            ],
            usage: { input_tokens: 20, output_tokens: 5 },
        },
    }),
    { call: { id: "toolu_1", name: "read", arguments: { path } } },
];

/** Claude's answer `text`, delivered whole, then the end of the query's turn. */
const whole = (text: string) => [
    say({
        type: "assistant",
        parent_tool_use_id: null,
        message: {
            id: "msg_2",
            model: "claude-fake",
            content: [{ type: "text", text }],
            usage: { input_tokens: 40, output_tokens: 3 },
        },
    }),
    say({ type: "result", subtype: "success", is_error: false, result: text }),
];

/** The realm's half of a sign-in: `credentials.json` as the calls see it. */
function credentialsFile() {
    const file = new Map<string, Kept>();
    const faked = fakeCalls({
        realm: {
            credential: ({ provider }) =>
                file.get(provider) ?? {
                    type: "refused",
                    why: { type: "not_signed_in", provider },
                    words: `Sign in to ${provider} first.`,
                },
            keep_credential: ({ provider, value }) => {
                file.set(provider, value);
                return null;
            },
        },
    });
    return { file, ...faked };
}

let requests = 0;

function login(agent: Woken) {
    const asked = chatActions({
        ...agent.config,
        signIns: signIns(agent.config.models),
    }).login;
    return (form: Omit<Login, "id">) => {
        requests += 1;
        const id = `00000000-0000-4000-8000-${String(requests).padStart(12, "0")}`;
        return asked(agent.by(from.client), { id, ...form } as Login);
    };
}

async function page(agent: Woken): Promise<ChatPage> {
    return agent.actions.read_chat(agent.by(from.client), {
        id: "00000000-0000-4000-8000-0000000000ff",
        chat: { with: 7 },
    });
}

/**
 * How long a query's end, its Node child's exit and its session's delete
 * may take: ten times the three seconds they take at most on a busy machine.
 */
const SESSION_ENDS_MS = 30_000;

/** The Claude Code sessions in the program's folder. */
function sessions(folder: string): string[] {
    const projects = join(folder, "projects");
    return existsSync(projects)
        ? readdirSync(projects).flatMap((project) =>
              readdirSync(join(projects, project)),
          )
        : [];
}

async function dumped(agent: Woken): Promise<string> {
    const tables = await agent.c.db.all<{ name: string }>(
        "SELECT name FROM sqlite_master WHERE type = 'table'",
    );
    const decoder = new TextDecoder();
    let text = "";
    for (const { name } of tables) {
        const rows = await agent.c.db.all<Record<string, unknown>>(
            `SELECT * FROM "${name}"`,
        );
        text += JSON.stringify(rows, (_key, value: unknown) =>
            value instanceof Uint8Array ? decoder.decode(value) : value,
        );
    }
    return text;
}

/**
 * An agent with the provider `claude` on the fake program running `runs`,
 * one list of steps per process, in a scratch realm folder. `more` adds to
 * the fake's script.
 */
async function claudeAgent(
    t: TestContext,
    runs: unknown[][],
    files: Record<string, string> = {},
    more: Record<string, unknown> = {},
) {
    const realm = mkdtempSync(join(tmpdir(), "hamlet-claude-"));
    t.after(() => rmSync(realm, { recursive: true, force: true }));
    const log = join(realm, "fake.jsonl");
    const script = join(realm, "script.json");
    writeFileSync(
        script,
        JSON.stringify({ log, token: TOKEN, refresh: REFRESH, runs, ...more }),
    );
    planted(t, { ...HOST, FAKE_CLAUDE_SCRIPT: script });
    const folder = join(realm, "claude");
    const { file, calls, sent } = credentialsFile();
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(files),
        models: (listed) =>
            withClaude(listed as MutableModels, { executable: FAKE, folder }),
    });
    const model = agent.config.models.getModels("claude")[0]?.id;
    await agent.actions.copies(agent.by(from.realm), {
        model: {
            version: 1,
            value: { model: `claude::${model}`, fast: false },
        },
    });
    let lines = 0;
    return {
        agent,
        realm,
        folder,
        file,
        sent,
        /** What each run of the fake program noted, in order. */
        noted: () =>
            readFileSync(log, "utf8")
                .trim()
                .split("\n")
                .map((line) => JSON.parse(line) as Record<string, unknown>),
        say: async (text: string) => {
            lines += 1;
            await agent.actions.submit(agent.by(from.realm), {
                id: `line-${lines}`,
                input: { type: "line", text },
            });
            await agent.idle();
        },
    };
}

/** The made-up subscription tokens the fake program keeps at its sign-in. */
const TOKEN = "made-up-subscription-token";
const REFRESH = "made-up-subscription-refresh";

/**
 * Tokens made up for one test: the host's scrub list outlives a test, so
 * tokens another test signed in with would be scrubbed already.
 */
function madeUpTokens(test: string) {
    return {
        token: `made-up-access-of-${test}`,
        refresh: `made-up-refresh-of-${test}`,
    };
}

/** The names of the values in `seen` that hold either of `tokens`. */
function holding(
    seen: Record<string, unknown>,
    tokens: { token: string; refresh: string },
): string[] {
    return Object.entries(seen)
        .filter(([, value]) =>
            [tokens.token, tokens.refresh].some((token) =>
                JSON.stringify(value).includes(token),
            ),
        )
        .map(([what]) => what);
}

/** The realm's marker that the chief signed in to Claude. */
function marked(claude: { file: Map<string, Kept> }): void {
    claude.file.set("claude", {
        type: "oauth",
        login: {
            access: "made-up-marker",
            refresh: "made-up-marker",
            expires: 4_294_967_295,
        },
    });
}

/** What this process writes to its stderr while the test runs. */
function stderrWrites(t: TestContext): string[] {
    const written: string[] = [];
    const write = process.stderr.write;
    process.stderr.write = ((chunk: string | Uint8Array, ...rest: never[]) => {
        written.push(Buffer.from(chunk).toString());
        return write.call(process.stderr, chunk, ...rest);
    }) as typeof process.stderr.write;
    t.after(() => {
        process.stderr.write = write;
    });
    return written;
}

test("a claude turn runs through the fake program", async (t) => {
    const claude = await claudeAgent(
        t,
        [[...readCall("notes.md"), ...streamed("Your notes say hi.")]],
        { [`${FAKE_ROOT}/notes.md`]: "hi" },
    );
    const { agent, folder, file } = claude;

    const ask = login(agent);
    const answers = [
        await ask({ start: { provider: "claude", method: "subscription" } }),
    ];
    const [started] = answers;
    assert.deepEqual(started, {
        type: "step",
        login: started?.login,
        step: {
            url: "https://claude.ai/oauth/authorize?fake=1",
            instructions:
                "Sign in to Claude in a browser on the computer Hamlet's server runs on. The sign-in finishes in Claude's own program; Hamlet never sees your login.",
        },
    });
    answers.push(
        await ask({ reply: { login: String(started?.login), value: "" } }),
    );
    assert.deepEqual(answers[1], { type: "done", login: started?.login });
    assert.equal(file.get("claude")?.type, "oauth", "the realm keeps a marker");

    await claude.say("What do my notes say?");
    const items = (await page(agent)).items;
    assert.equal(
        items.findLast((item) => item.kind === "assistant")?.text,
        "Your notes say hi.",
    );

    const [signedIn, asked, read] = claude.noted();
    assert.deepEqual(signedIn?.login, ["auth", "login", "--claudeai"]);
    assert.equal(asked?.run, 0);
    assert.match(JSON.stringify(asked?.prompt), /What do my notes say\?/);
    assert.deepEqual(read?.result, {
        content: [{ type: "text", text: "hi" }],
        isError: false,
    });

    // Hamlet spawns the sign-in itself, so it sets every Claude variable.
    assert.deepEqual(
        Object.keys(signedIn?.env as object)
            .filter((name) => name.startsWith("CLAUDE"))
            .sort(),
        [
            "CLAUDE_AGENT_SDK_CLIENT_APP",
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
            "CLAUDE_CODE_MAX_MCP_DESCRIPTION_LENGTH",
            "CLAUDE_CODE_RESUME_INTERRUPTED_TURN_MAX_AGE_MS",
            "CLAUDE_CODE_SILENT_TURN_REMINDER",
            "CLAUDE_CONFIG_DIR",
        ],
    );
    for (const run of [signedIn, asked]) {
        const env = run?.env as Record<string, string>;
        assert.deepEqual(
            Object.keys(env).filter((name) =>
                /^(ANTHROPIC_|HAMLET_)/.test(name),
            ),
            [],
        );
        assert.equal(env.CLAUDE_CONFIG_DIR, folder);
        assert.equal(env.CLAUDE_CODE_OAUTH_TOKEN, undefined);
        for (const value of Object.values(HOST))
            assert.ok(
                !Object.values(env).includes(value),
                `the child has ${value}`,
            );
    }
    assert.equal(statSync(folder).mode & 0o777, 0o700);
    // The query ends with its turn, and removes the session it wrote.
    await until(() => sessions(folder).length === 0, SESSION_ENDS_MS);

    assert.match(
        readFileSync(join(folder, ".credentials.json"), "utf8"),
        new RegExp(TOKEN),
        "the program keeps its own login",
    );
    const seen = {
        database: await dumped(agent),
        log: agent.reported,
        events: agent.events,
        "sign-in answers": answers,
        "chat page": items,
        "realm calls": claude.sent,
        "the realm's credentials": [...file.values()],
    };
    assert.deepEqual(holding(seen, { token: TOKEN, refresh: REFRESH }), []);
    await agent.stop();
});

test("the next input resumes a session the SDK reads as the agent's history", async (t) => {
    const claude = await claudeAgent(t, [whole("Hi."), whole("Again, hi.")]);
    const { agent, folder, realm } = claude;
    marked(claude);

    await claude.say("Hello");
    await claude.say("Again");
    assert.equal(
        (await page(agent)).items.findLast((item) => item.kind === "assistant")
            ?.text,
        "Again, hi.",
    );
    const [, again] = claude.noted();
    assert.equal(again?.run, 1);
    assert.match(JSON.stringify(again?.prompt), /Again/);
    assert.notEqual(again?.resume, null);

    // The session was removed with its query, so the SDK reads the copy
    // the fake program noted, at the same place in another Claude folder.
    const copy = join(realm, "copy");
    const written = join(copy, String(again?.file));
    mkdirSync(dirname(written), { recursive: true });
    writeFileSync(written, String(again?.resumed));
    // Planted with the host's environment, so put back after the test.
    process.env.CLAUDE_CONFIG_DIR = copy;
    const history = await getSessionMessages(String(again?.resume), {
        dir: join(folder, "work"),
    });
    assert.deepEqual(
        // A message's id is made up as it is written.
        history.map(({ type, message }) => {
            const { id: _, ...said } = message as Record<string, unknown>;
            return [type, said];
        }),
        [
            ["user", { role: "user", content: "Hello" }],
            [
                "assistant",
                {
                    type: "message",
                    role: "assistant",
                    model: agent.config.models.getModels("claude")[0]?.id,
                    content: [{ type: "text", text: "Hi." }],
                    stop_reason: "end_turn",
                    stop_sequence: null,
                    usage: { input_tokens: 0, output_tokens: 0 },
                },
            ],
        ],
    );
    await until(() => sessions(folder).length === 0, SESSION_ENDS_MS);
    await agent.stop();
});

test("a turn Claude bills beyond the plan fails", async (t) => {
    const claude = await claudeAgent(t, [
        [
            say({
                type: "rate_limit_event",
                rate_limit_info: { status: "rejected", isUsingOverage: true },
            }),
            ...whole("Billed."),
        ],
    ]);
    marked(claude);
    await claude.say("Hello");
    const items = (await page(claude.agent)).items;
    assert.ok(
        !items.some((item) => item.text === "Billed."),
        "the billed answer is not shown",
    );
    assert.match(
        JSON.stringify(items),
        /Claude is billing this beyond your plan, to extra usage, so Hamlet stopped\. It goes on once your plan's limit resets\./,
    );
    await until(() => sessions(claude.folder).length === 0, SESSION_ENDS_MS);
    await claude.agent.stop();
});

test("a failed sign-in's words hold none of the program's tokens", async (t) => {
    const tokens = madeUpTokens("failed-sign-in");
    const claude = await claudeAgent(
        t,
        [],
        {},
        {
            ...tokens,
            signInFails: `debug: kept ${tokens.token} and ${tokens.refresh}`,
        },
    );
    const ask = login(claude.agent);
    const started = await ask({
        start: { provider: "claude", method: "subscription" },
    });
    const failed = await ask({
        reply: { login: String(started.login), value: "" },
    });
    assert.equal(failed.type, "failed");
    assert.match(
        JSON.stringify(failed),
        /Claude's sign-in failed\. debug: kept \[REDACTED\] and \[REDACTED\]/,
    );
    const seen = {
        "sign-in answers": [started, failed],
        log: claude.agent.reported,
        events: claude.agent.events,
        database: await dumped(claude.agent),
    };
    assert.deepEqual(holding(seen, tokens), []);
    await claude.agent.stop();
});

test("a program that dies with its tokens on stderr shows them nowhere", async (t) => {
    const tokens = madeUpTokens("dying-program");
    const written = stderrWrites(t);
    const claude = await claudeAgent(t, [
        [{ die: `fatal: bad token ${tokens.token} ${tokens.refresh}\n` }],
    ]);
    marked(claude);
    // The login the program kept when the chief signed in, before this host started.
    writeFileSync(
        join(claude.folder, ".credentials.json"),
        JSON.stringify({
            claudeAiOauth: {
                accessToken: tokens.token,
                refreshToken: tokens.refresh,
            },
        }),
    );
    await claude.say("Hello");
    await until(() => written.some((line) => line.startsWith("claude: ")));
    const items = (await page(claude.agent)).items;
    assert.match(
        String(items.findLast((item) => item.kind === "error")?.text),
        /fatal: bad token \[REDACTED\] \[REDACTED\]/,
    );
    const seen = {
        chat: items,
        events: claude.agent.events,
        log: claude.agent.reported,
        database: await dumped(claude.agent),
        "relayed stderr": written.filter((line) => line.startsWith("claude: ")),
    };
    assert.deepEqual(holding(seen, tokens), []);
    await claude.agent.stop();
});

test("a tool result's image is in the session the next input resumes", async (t) => {
    const claude = await claudeAgent(
        t,
        [[...readCall("pic.gif"), ...streamed("A picture.")], whole("Still.")],
        { [`${FAKE_ROOT}/pic.gif`]: "GIF89a\u0001\u0000\u0001\u0000" },
    );
    marked(claude);
    await claude.say("Look at pic.gif");
    await claude.say("What was in it?");
    const [, again] = claude
        .noted()
        .filter((note) => note.prompt !== undefined);
    const results = String(again?.resumed)
        .trim()
        .split("\n")
        .flatMap((line) => {
            const content = (
                JSON.parse(line) as { message?: { content?: unknown } }
            ).message?.content;
            return Array.isArray(content) ? content : [];
        })
        .filter((block) => block.tool_use_id === "toolu_1");
    assert.deepEqual(
        results.map((block) => block.content),
        [
            [
                {
                    type: "image",
                    source: {
                        type: "base64",
                        media_type: "image/gif",
                        data: "R0lGODlhAQABAA==",
                    },
                },
            ],
        ],
    );
    await claude.agent.stop();
});

test("loading the models loads none of the Claude packages", () => {
    const models = join(import.meta.dirname, "..", "src", "agent", "models.ts");
    const resolved = execFileSync(
        process.execPath,
        [
            "--input-type=module",
            "--no-warnings",
            "--eval",
            `import { registerHooks } from "node:module";
const urls = [];
registerHooks({ resolve: (specifier, context, next) => {
    const found = next(specifier, context);
    urls.push(found.url);
    return found;
} });
await import(${JSON.stringify(models)});
process.stdout.write(JSON.stringify(urls));`,
        ],
        { encoding: "utf8" },
    );
    const urls = JSON.parse(resolved) as string[];
    assert.ok(urls.some((url) => url.endsWith("/claude.ts")));
    assert.deepEqual(
        urls.filter((url) =>
            /claude-agent-sdk|modelcontextprotocol|cc-session-io/.test(url),
        ),
        [],
    );
});
