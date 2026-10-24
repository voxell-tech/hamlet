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
import {
    allowedStreams,
    type Gate,
    modelProvider,
    selectedModel,
} from "../src/models.ts";
import type { Usage } from "../src/protocol.ts";
import { providers } from "../src/provider-set.ts";

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

test("reads a script of shared responses and refuses malformed agent keys", async () => {
    const folder = mkdtempSync(join(tmpdir(), "hamlet-pi-faux-"));
    folders.push(folder);
    const file = join(folder, "faux.json");
    const shared = {
        responses: ["Hello", { thinking: "Consider", text: "Yes" }],
        tokensPerSecond: 40,
    };
    writeFileSync(file, JSON.stringify(shared));
    expect(await readScript(file)).toEqual(shared);
    writeFileSync(
        file,
        JSON.stringify({ ...shared, models: ["faux-1", "faux-2"] }),
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
    const other = "server-only-other-key";
    const charged: Usage[] = [];
    const faux = scriptedProvider(
        {
            responses: [
                {
                    text: `Text ${key} ${other}`,
                    thinking: `Thought ${key} ${other}`,
                    calls: [{ name: "read", arguments: { path: key, other } }],
                },
                { error: `Failure ${key} ${other}` },
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
                scrub: async <T>(value: T): Promise<T> => {
                    await sleep(2);
                    return JSON.parse(
                        JSON.stringify(value).replaceAll(other, "[REDACTED]"),
                    ) as T;
                },
            }),
    );
    const [model] = faux.getModels();
    if (model === undefined) throw new Error("No faux model");
    const stream = faux.streamSimple(model, PROMPT, { sessionId: "s" });
    for await (const event of stream) {
        expect(JSON.stringify(event)).not.toContain(other);
        expect(JSON.stringify(event)).not.toContain(
            JSON.stringify(key).slice(1, -1),
        );
    }
    const answer = await stream.result();
    expect(charged).toHaveLength(1);
    expect(answer.content).toMatchObject([
        { type: "thinking", thinking: "Thought [REDACTED] [REDACTED]" },
        { type: "text", text: "Text [REDACTED] [REDACTED]" },
        {
            type: "toolCall",
            arguments: { path: "[REDACTED]", other: "[REDACTED]" },
        },
    ]);
    const failed = await faux
        .streamSimple(model, PROMPT, { sessionId: "s" })
        .result();
    expect(charged).toHaveLength(2);
    expect(failed.errorMessage).toBe("Failure [REDACTED] [REDACTED]");
    expect(charged[1]?.estimatedTokens).toBe(0);
});

test("keeps no provider output when the server cannot scrub it", async () => {
    const secret = "server-only-unavailable-key";
    const faux = scriptedProvider(
        { responses: [secret] },
        () => "",
        (streams) =>
            allowedStreams(streams, {
                attempt: async () => ({ ok: true, key: "attempt" }),
                charge: async () => {},
                scrub: async () => {
                    throw new Error(secret);
                },
            }),
    );
    const model = faux.getModels()[0];
    if (model === undefined) throw new Error("No faux model");
    const stream = faux.streamSimple(model, PROMPT, { sessionId: "s" });
    for await (const event of stream)
        expect(JSON.stringify(event)).not.toContain(secret);
    const answer = await stream.result();
    expect(answer.stopReason).toBe("error");
    expect(answer.errorMessage).toBe(
        "The server could not scrub model output.",
    );
});

test("a selection is made as its provider lists the model, at the server's price and window", () => {
    const available = providers();
    const listed = available.find((p) => p.id === "openai")?.getModels();
    const [called, other] = listed ?? [];
    if (called === undefined || other === undefined)
        throw new Error("No OpenAI models");
    const price = { input: 3, output: 7, cacheRead: 0.5, cacheWrite: 4 };
    const switched = selectedModel(
        called,
        {
            model: { provider: "openai", id: other.id },
            contextWindow: 1234,
            price,
        },
        available,
    );
    expect(switched).toMatchObject({
        id: other.id,
        api: other.api,
        baseUrl: other.baseUrl,
        contextWindow: 1234,
        maxTokens: other.maxTokens,
        cost: price,
    });
    const endpoint = selectedModel(
        called,
        {
            model: {
                provider: "local",
                id: "small",
                baseUrl: "http://localhost:8080/v1",
            },
            price,
        },
        available,
    );
    expect(endpoint).toMatchObject({
        provider: "local",
        id: "small",
        api: "openai-completions",
        baseUrl: "http://localhost:8080/v1",
        cost: price,
    });
});

test("a model of a provider pi has no built-in for needs an address", () => {
    const available = providers();
    const [called] =
        available.find((p) => p.id === "openai")?.getModels() ?? [];
    if (called === undefined) throw new Error("No OpenAI models");
    const unknown = { provider: "elsewhere", id: "large" };
    const refusal =
        "The model elsewhere::large has no address: pi has no provider elsewhere of its own, and the server gave no base URL.";
    expect(() =>
        selectedModel(
            called,
            {
                model: unknown,
                price: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
            },
            available,
        ),
    ).toThrow(refusal);
    const gate: Gate = {
        attempt: async () => ({ ok: false, words: "unused" }),
        charge: async () => {},
        scrub: async (value) => value,
    };
    expect(() => modelProvider(unknown, gate, available)).toThrow(refusal);
    expect(
        modelProvider(
            { ...unknown, baseUrl: "http://localhost:8080/v1" },
            gate,
            available,
        ).getModels()[0]?.baseUrl,
    ).toBe("http://localhost:8080/v1");
});
