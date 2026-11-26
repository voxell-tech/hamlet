import assert from "node:assert/strict";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { test } from "node:test";
import { createModels } from "@earendil-works/pi-ai";
import { checkConnection } from "../src/agent/models.ts";
import { servingEndpoints } from "../src/agent/providers.ts";
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
