import {
    type AssistantMessage,
    createAssistantMessageEventStream,
    createModels,
    normalizeContext,
    type StreamFunction,
} from "@earendil-works/pi-ai";
import { createProvider } from "@earendil-works/pi-ai/models";
import { expect, test } from "vitest";
import { compatibleModel, type Gate, modelProvider } from "../src/models.ts";

const gate: Gate = {
    attempt: async () => ({ ok: false, words: "no calls in this test" }),
    charge: async () => {},
    scrub: async (value) => value,
};

test("an endpoint made again for a second model still lists the first", () => {
    const models = createModels();
    const sonnet = { provider: "anthropic", id: "claude-sonnet-5-5" };
    const opus = { provider: "anthropic", id: "claude-opus-5-5" };
    models.setProvider(modelProvider(sonnet, gate));
    expect(models.getModel("anthropic", "claude-opus-5-5")).toBeUndefined();
    const first = models.getModel("anthropic", "claude-sonnet-5-5");
    models.setProvider(
        modelProvider(
            opus,
            gate,
            undefined,
            models.getProvider("anthropic")?.getModels(),
        ),
    );
    expect(models.getModel("anthropic", "claude-opus-5-5")).toBeDefined();
    // The same model, so what was set on it, such as its context window, stays.
    expect(models.getModel("anthropic", "claude-sonnet-5-5")).toBe(first);
});

test("a model of an endpoint pi does not know is served at the server's address", () => {
    const models = createModels();
    const at = "http://127.0.0.1:9/v1";
    models.setProvider(
        modelProvider({ provider: "local", id: "a", baseUrl: at }, gate),
    );
    models.setProvider(
        modelProvider(
            { provider: "local", id: "b", baseUrl: at },
            gate,
            undefined,
            models.getProvider("local")?.getModels(),
        ),
    );
    expect(models.getModels("local").map((m) => m.id)).toEqual(["a", "b"]);
});

test("a conversation switched to a model its provider does not list yet calls that model", async () => {
    // The provider lists only `a`, the model the conversation was made with.
    const called: string[] = [];
    const answer: StreamFunction = (model) => {
        called.push(model.id);
        const out = createAssistantMessageEventStream();
        const message: AssistantMessage = {
            role: "assistant",
            content: [{ type: "text", text: "Done." }],
            api: model.api,
            provider: model.provider,
            model: model.id,
            usage: {
                input: 0,
                output: 0,
                cacheRead: 0,
                cacheWrite: 0,
                totalTokens: 0,
                cost: {
                    input: 0,
                    output: 0,
                    cacheRead: 0,
                    cacheWrite: 0,
                    total: 0,
                },
            },
            stopReason: "stop",
            timestamp: 0,
        };
        out.push({ type: "done", reason: "stop", message });
        out.end(message);
        return out;
    };
    const acme = createProvider({
        id: "acme",
        models: [compatibleModel("acme", "a", "http://127.0.0.1:9/v1")],
        auth: {
            apiKey: {
                name: "acme key",
                resolve: async () => ({ auth: { apiKey: "test" } }),
            },
        },
        api: { stream: answer, streamSimple: answer },
    });
    // The server's answer to each call names the model the agent runs now.
    const switched: Gate = {
        attempt: async () => ({
            ok: true,
            key: "k",
            selection: {
                model: { provider: "acme", id: "b" },
                price: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
            },
        }),
        charge: async () => {},
        scrub: async (value) => value,
    };
    const models = createModels();
    models.setProvider(
        modelProvider({ provider: "acme", id: "a" }, switched, [acme]),
    );
    const made = models.getModel("acme", "a");
    if (made === undefined) throw new Error("The model it was made with");
    const reply = await models
        .getProvider("acme")
        ?.streamSimple(made, normalizeContext({ messages: [] }), {
            sessionId: "s",
        })
        .result();
    expect(reply?.errorMessage).toBeUndefined();
    expect(called).toEqual(["b"]);
});
