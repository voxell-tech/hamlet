import { readFile } from "node:fs/promises";
import { Type } from "@earendil-works/pi-ai";
import { defineExtension, defineTool } from "@earendil-works/pi-durable";
import { describe, expect, it, vi } from "vitest";
import { localDay } from "../src/home/clock.ts";
import { HOME } from "../src/record.ts";
import { SettingsDoc } from "../src/settings.ts";

import { openaiFake } from "./openai-fake.ts";
import {
    context,
    fauxModels,
    ledgerOf,
    say,
    scratch,
    testHome,
} from "./support.ts";

describe("the home's model providers", () => {
    it("retries a busy endpoint and keeps overrides out of the home", async () => {
        const endpoint = await openaiFake();
        endpoint.fail(1);
        const home = await testHome(scratch(), {
            env: {
                HAMLET_MODEL: "local::org/large",
                HAMLET_BASE_URL: endpoint.url,
                HAMLET_API_KEY: "memory-only-key",
            },
        });
        expect(await (await say(home, "Hello.")).wait(context)).toMatchObject({
            status: "done",
        });
        expect(endpoint.requests).toHaveLength(2);
        for (const file of [
            home.layout.credentials,
            home.layout.models,
            home.layout.storage,
        ]) {
            expect(
                (await readFile(file)).includes(Buffer.from("memory-only-key")),
            ).toBe(false);
            expect(
                (await readFile(file)).includes(Buffer.from(endpoint.url)),
            ).toBe(false);
        }
        expect(await home.harness.snapshot(SettingsDoc, context)).toMatchObject(
            {
                model: "local::qwen3.6:35b-a3b",
                endpoints: [{ baseUrl: "http://localhost:11434/v1" }],
            },
        );
        await home.credentials.modify("local", async () => ({
            type: "api_key",
            key: "forgotten",
        }));
        await home.modelHome.editSettings({ endpoints: [] }, context);
        expect(await home.credentials.read("local")).toBeUndefined();
    });

    it("resolves a cut-off turn's stored model with its endpoint down after restart", async () => {
        const endpoint = await openaiFake();
        endpoint.hold();
        const dir = scratch();
        const options = {
            env: {
                HAMLET_MODEL: "local::org/large",
                HAMLET_BASE_URL: endpoint.url,
            },
        };
        let home = await testHome(dir, options);
        const submission = await say(home, "Keep this turn.");
        await vi.waitFor(() => expect(endpoint.requests).toHaveLength(1));
        await home.close(context);
        await endpoint.close();
        home = await testHome(dir, options);
        expect(home.models.getModel("local", "org/large")).toMatchObject({
            contextWindow: 32_000,
        });
        const resumed = await home.harness.submission(submission.id, context);
        expect(await resumed?.wait(context)).toMatchObject({
            status: "unanswered",
            reason: "model_error",
        });
    }, 20_000);

    it("runs a slash-containing model id where its endpoint says and records billed final-chunk usage", async () => {
        const endpoint = await openaiFake();
        const home = await testHome(scratch(), {
            env: {
                HAMLET_MODEL: "local::org/large",
                HAMLET_BASE_URL: endpoint.url,
                HAMLET_API_KEY: "secret-key",
            },
        });
        expect(await (await say(home, "Hello.")).wait(context)).toMatchObject({
            status: "done",
        });
        expect(endpoint.requests[0]?.model).toBe("org/large");
        expect(endpoint.keys).toContain("Bearer secret-key");
        expect(
            await ledgerOf(home, HOME, localDay(home.now()), context),
        ).toEqual([
            expect.objectContaining({
                model: "local::org/large",
                host: "127.0.0.1",
                tokens: 120,
                cached: 30,
                reasoning: 5,
                micros: 1500,
                billed: 0.0015,
            }),
        ]);
    });
});

it("estimates every unreported request from its full history and tool definitions", async () => {
    const endpoint = await openaiFake();
    endpoint.listing({ data: [{ id: "org/large", context_length: 128_000 }] });
    endpoint.usage(undefined);
    const home = await testHome(scratch(), {
        env: {
            HAMLET_MODEL: "local::org/large",
            HAMLET_BASE_URL: endpoint.url,
            HAMLET_REFLECT_EVERY: "0",
        },
        parts: [
            () => ({
                extension: defineExtension({
                    name: "test.large-tool",
                    tools: [
                        defineTool({
                            name: "large_tool",
                            description: "t".repeat(40000),
                            parameters: Type.Object({}),
                            execute: async () => ({}),
                        }),
                    ],
                }),
            }),
        ],
    });
    await (await say(home, "a".repeat(20000))).wait(context);
    await (await say(home, "b".repeat(20000))).wait(context);
    const lines = await ledgerOf(home, HOME, localDay(home.now()), context);
    expect(lines).toMatchObject([
        { what: "model", estimated: true },
        { what: "model", estimated: true },
    ]);
    const second = lines[1];
    if (second === undefined || second.what === "reset")
        throw new Error("missing second charge");
    // The chief lines and offered tool contain 80,000 bytes, at four bytes per token.
    expect(second.tokens).toBeGreaterThan(20000);
});

it("keeps a refused attempt at zero spend when the endpoint reported no usage", async () => {
    const endpoint = await openaiFake();
    endpoint.fail(1);
    const home = await testHome(scratch(), {
        env: {
            HAMLET_MODEL: "local::org/large",
            HAMLET_BASE_URL: endpoint.url,
            HAMLET_REFLECT_EVERY: "0",
        },
    });
    await (await say(home, "Hello.")).wait(context);
    expect(
        await ledgerOf(home, HOME, localDay(home.now()), context),
    ).toMatchObject([
        {
            what: "attempt",
            tokens: 0,
            micros: 0,
            estimated: false,
            stopReason: "error",
        },
        { what: "model", tokens: 120, micros: 1500, estimated: false },
    ]);
});

it("restores the stored endpoint's address and fallback price after a base URL override ends", async () => {
    const endpoint = await openaiFake();
    const dir = scratch();
    const home = await testHome(dir, {
        env: {
            HAMLET_MODEL: "local::org/large",
            HAMLET_BASE_URL: endpoint.url,
            HAMLET_REFLECT_EVERY: "0",
        },
    });
    const savedUrl = endpoint.url.replace("127.0.0.1", "[::ffff:127.0.0.1]");
    await home.modelHome.editSettings(
        {
            model: "local::org/large",
            endpoints: [{ name: "local", baseUrl: savedUrl }],
        },
        context,
    );
    await home.close(context);
    const reopened = await testHome(dir, {
        env: { HAMLET_MODEL: "local::org/large", HAMLET_REFLECT_EVERY: "0" },
    });
    expect(reopened.models.getProvider("local")?.baseUrl).toBe(savedUrl);
    expect(reopened.models.getModel("local", "org/large")).toMatchObject({
        baseUrl: savedUrl,
        cost: { input: 1, output: 1 },
    });
});

it("refuses credentials in an endpoint address without changing settings", async () => {
    const { models } = fauxModels();
    const home = await testHome(scratch(), { models });
    const before = await home.harness.snapshot(SettingsDoc, context);
    await expect(
        home.modelHome.editSettings(
            { endpoints: [{ name: "private", baseUrl: "http://u:p@host/v1" }] },
            context,
        ),
    ).resolves.toMatchObject({
        ok: false,
        error: { kind: "address-has-credentials" },
    });
    expect(await home.harness.snapshot(SettingsDoc, context)).toEqual(before);
});

it("charges a provider's explicit zero bill instead of the catalogue price", async () => {
    const endpoint = await openaiFake();
    endpoint.usage({ prompt_tokens: 100, completion_tokens: 20, cost: 0 });
    const home = await testHome(scratch(), {
        env: {
            HAMLET_MODEL: "local::org/large",
            HAMLET_BASE_URL: endpoint.url,
            HAMLET_REFLECT_EVERY: "0",
        },
    });
    await (await say(home, "Free call.")).wait(context);
    expect(
        await ledgerOf(home, HOME, localDay(home.now()), context),
    ).toMatchObject([{ what: "model", tokens: 120, micros: 0, billed: 0 }]);
});

it.each(["localhost:11434/v1", "http://chief:secret@127.0.0.1:1/v1"])(
    "books catalogue prices with an unusable default override %s",
    async (baseUrl) => {
        const endpoint = await openaiFake();
        endpoint.usage({ prompt_tokens: 100, completion_tokens: 20 });
        const dir = scratch();
        const seed = await testHome(dir, { models: fauxModels().models });
        expect(
            await seed.modelHome.editSettings(
                {
                    endpoints: [
                        { name: "local", baseUrl: "http://127.0.0.1:1/v1" },
                        { name: "backup", baseUrl: endpoint.url },
                    ],
                },
                context,
            ),
        ).toMatchObject({ ok: true });
        await seed.close(context);
        const home = await testHome(dir, {
            env: {
                HAMLET_MODEL: "local::tiny",
                HAMLET_BASE_URL: baseUrl,
                HAMLET_REFLECT_EVERY: "0",
            },
        });
        const { LimitsDoc } = await import("../src/limits.ts");
        await home.harness.commit(async (tx) => {
            (await tx.doc(LimitsDoc, HOME, HOME)).model = "backup::org/large";
        }, context);
        expect(
            await home.modelHome.editCatalogue(
                "backup::org/large",
                { price: { input: 2, output: 3 } },
                context,
            ),
        ).toMatchObject({ ok: true });
        expect(await (await say(home, "Hello.")).wait(context)).toMatchObject({
            status: "done",
        });
        expect(
            await ledgerOf(home, HOME, localDay(home.now()), context),
        ).toMatchObject([
            {
                what: "model",
                model: "backup::org/large",
                tokens: 120,
                micros: 260,
            },
        ]);
    },
);
