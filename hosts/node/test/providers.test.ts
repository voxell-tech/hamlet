import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { test } from "node:test";
import {
    createFauxCore,
    createModels,
    createProvider,
} from "@earendil-works/pi-ai";
import type { ProviderView } from "../src/agent/messages/agent.ts";
import {
    checkConnection,
    providerViews,
    reached,
} from "../src/agent/models.ts";
import { hostProviders, servingEndpoints } from "../src/agent/providers.ts";
import { until } from "./agent-testing.ts";

/** A loopback server listing `org/large` with a context of 32000 tokens, as the acceptance suite's. */
async function listing(t: { after: (f: () => void) => void }): Promise<string> {
    const server: Server = createServer((request, response) => {
        response.setHeader("content-type", "application/json");
        response.end(
            request.url?.endsWith("/models")
                ? JSON.stringify({
                      data: [{ id: "org/large", context_length: 32000 }],
                  })
                : "{}",
        );
    });
    await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
    t.after(() => server.close());
    return `http://127.0.0.1:${(server.address() as AddressInfo).port}/v1`;
}

test("an endpoint the settings name is served with the copy's model, its list is asked for, and it goes when they drop it", async (t) => {
    const base_url = await listing(t);
    const models = createModels();
    const serve = servingEndpoints(models);
    const settings = (names: readonly string[]) => ({
        reflect_every: 0,
        allow_net: false,
        endpoints: names.map((name) => ({ name, base_url })),
    });

    serve({
        settings: settings(["local"]),
        model: { model: "local::org/small" },
    });
    assert.ok(models.getModel("local", "org/small"), "the copy's model");
    await until(
        () => models.getModel("local", "org/large")?.contextWindow === 32000,
    );
    assert.deepEqual(await checkConnection(models, "local::org/large"), {
        type: "answered",
        words: "Connected, 2 model(s)",
        models: 2,
    });
    const login = models.getProvider("local")?.auth.apiKey?.login;
    assert.ok(login, "a key sign-in");
    let asked: unknown;
    await login({
        prompt: async (prompt) => {
            asked = prompt;
            return "sk-pasted";
        },
        notify: () => {},
        signal: AbortSignal.timeout(5000),
    });
    assert.deepEqual(asked, { type: "secret", message: "Paste your key." });

    serve({ settings: settings([]) });
    assert.equal(models.getProvider("local"), undefined);
});

test("only an endpoint or a provider the realm holds a sign-in for is reached, as today's catalogue lists them", async (t) => {
    const base_url = await listing(t);
    const models = createModels();
    for (const id of ["kept", "other"]) {
        const core = createFauxCore({ provider: id });
        models.setProvider(
            createProvider({
                id,
                models: core.models,
                api: { stream: core.stream, streamSimple: core.streamSimple },
                auth: { apiKey: { name: id, resolve: async () => undefined } },
            }),
        );
    }
    servingEndpoints(models)({
        settings: {
            reflect_every: 0,
            allow_net: false,
            endpoints: [{ name: "local", base_url }],
        },
    });
    const realm = {
        credential: async ({ provider }: { provider: string }) =>
            provider === "kept"
                ? ({ type: "key", key: "sk-made-up" } as const)
                : ({
                      type: "refused",
                      why: { type: "not_signed_in", provider },
                      words: `no one has signed in to ${provider}`,
                  } as const),
    };

    assert.deepEqual([...(await reached(models, realm))].sort(), [
        "kept",
        "local",
    ]);
});

/** The providers today's server lists in its recorded sign-in session. */
function todaysProviders(): ProviderView[] {
    const frames = readFileSync(
        new URL(
            "../../../crates/hamlet_protocol/frames/login-key.jsonl",
            import.meta.url,
        ),
        "utf8",
    );
    for (const line of frames.split("\n")) {
        if (line === "") continue;
        const { message } = JSON.parse(line) as {
            message: { changes?: { providers?: ProviderView[] }[] };
        };
        const listed = message.changes?.find((change) => change.providers);
        if (listed?.providers !== undefined) return listed.providers;
    }
    throw new Error("today's session lists no providers");
}

test("the host's providers are today's, by today's names", async () => {
    const models = createModels();
    for (const provider of hostProviders()) models.setProvider(provider);
    const views = await providerViews(models, {
        credential: async ({ provider }) => ({
            type: "refused",
            why: { type: "not_signed_in", provider },
            words: `nothing is signed in to ${provider}`,
        }),
    });
    // The realm's endpoint and the Claude program's provider are not the
    // host's built-in ones.
    const shown = (providers: readonly ProviderView[]) =>
        providers
            .filter(({ id }) => id !== "local" && id !== "claude")
            .map(({ id, name, label, methods }) => ({
                id,
                name,
                label,
                methods,
            }));

    assert.deepEqual(shown(views), shown(todaysProviders()));
});
