import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import {
    createAssistantMessageEventStream,
    normalizeContext,
    type SimpleStreamOptions,
    type StreamFunction,
} from "@earendil-works/pi-ai";
import { afterEach, expect, test } from "vitest";
import { readScript, type Script, scriptedProvider } from "../src/faux.ts";
import type { Usage } from "../src/link.ts";
import { allowedStreams } from "../src/models.ts";

const folders: string[] = [];

afterEach(() => {
    for (const folder of folders.splice(0))
        rmSync(folder, { recursive: true, force: true });
});

const PROMPT = normalizeContext({ messages: [] });

/** The faux provider of `script`, each call routed by its session as the agent it names and allowed after the next of `delays` milliseconds, as the server's answers may come late. */
function provider(script: Script, delays: number[]) {
    const late = (stream: StreamFunction<string, SimpleStreamOptions>) =>
        ((model, context, options) => {
            const out = createAssistantMessageEventStream();
            const delay = delays.shift() ?? 0;
            void (async () => {
                await sleep(delay);
                const source = stream(model, context, options);
                for await (const event of source) out.push(event);
                out.end(await source.result());
            })();
            return out;
        }) satisfies StreamFunction<string, SimpleStreamOptions>;
    const faux = scriptedProvider(
        script,
        (options) => options?.sessionId ?? "",
        (streams) => ({
            stream: late(streams.stream),
            streamSimple: late(streams.streamSimple),
        }),
    );
    const [model] = faux.getModels();
    if (model === undefined) throw new Error("No faux model");
    return async (agent: string) => {
        const answer = await faux
            .streamSimple(model, PROMPT, { sessionId: agent })
            .result();
        return answer.errorMessage ?? answer.content;
    };
}

test("keeps old scripts and refuses malformed agent keys", async () => {
    const folder = mkdtempSync(join(tmpdir(), "hamlet-pi-faux-"));
    folders.push(folder);
    const file = join(folder, "faux.json");
    const old = {
        responses: ["Hello", { thinking: "Consider", text: "Yes" }],
        tokensPerSecond: 40,
    };
    writeFileSync(file, JSON.stringify(old));
    expect(await readScript(file)).toEqual(old);
    writeFileSync(
        file,
        JSON.stringify({ ...old, models: ["faux-1", "faux-2"] }),
    );
    const script = await readScript(file);
    const models = scriptedProvider(
        script,
        () => "",
        (streams) => streams,
    ).getModels();
    expect(models.map((model) => model.id)).toEqual(["faux-1", "faux-2"]);
    writeFileSync(file, JSON.stringify({ agents: { "home:mayor": ["No"] } }));
    await expect(readScript(file)).rejects.toThrow("not a faux script");
});

test("takes each list's steps in the order calls are made, whenever the server allows them", async () => {
    const ask = provider(
        {
            agents: { "home:0": ["first", "second"], "home:1": ["scout"] },
            responses: ["fallback"],
        },
        // The first call is allowed last.
        [50, 0, 0],
    );
    // Made together: each takes its step as it is made.
    const answers = await Promise.all([
        ask("home:0"),
        ask("home:1"),
        ask("home:0"),
    ]);
    expect(answers).toEqual([
        [{ type: "text", text: "first" }],
        [{ type: "text", text: "scout" }],
        [{ type: "text", text: "second" }],
    ]);
    // A list that ran out fails, and never falls back to `responses`.
    expect(await ask("home:0")).toBe("No more faux responses queued");
    expect(await ask("home:2")).toEqual([{ type: "text", text: "fallback" }]);
});

test("charges before returning an answer and scrubs credentials from kept text, thinking, arguments and failures", async () => {
    const key = 'sk-test-"private';
    const charged: Usage[] = [];
    const faux = scriptedProvider(
        {
            responses: [
                {
                    text: `Text ${key}`,
                    thinking: `Thought ${key}`,
                    calls: [{ name: "read", arguments: { path: key } }],
                },
                { error: `Failure ${key}` },
            ],
        },
        () => "",
        (streams) =>
            allowedStreams(streams, {
                attempt: async () => ({
                    ok: true,
                    key: "attempt",
                    apiKey: key,
                }),
                charge: async (_id, usage) => {
                    charged.push(usage);
                },
            }),
    );
    const [model] = faux.getModels();
    if (model === undefined) throw new Error("No faux model");
    const answer = await faux
        .streamSimple(model, PROMPT, { sessionId: "s" })
        .result();
    expect(charged).toHaveLength(1);
    expect(answer.content).toMatchObject([
        { type: "thinking", thinking: "Thought [REDACTED]" },
        { type: "text", text: "Text [REDACTED]" },
        { type: "toolCall", arguments: { path: "[REDACTED]" } },
    ]);
    const failed = await faux
        .streamSimple(model, PROMPT, { sessionId: "s" })
        .result();
    expect(charged).toHaveLength(2);
    expect(failed.errorMessage).toBe("Failure [REDACTED]");
    expect(charged[1]?.estimatedTokens).toBe(0);
});
