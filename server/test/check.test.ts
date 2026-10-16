import { describe, expect, it, vi } from "vitest";
import { checkConnection, checkModel, plainly } from "../src/models.ts";
import { SettingsDoc } from "../src/settings.ts";
import { openaiFake } from "./openai-fake.ts";
import { context, fauxModels, scratch, testHome } from "./support.ts";

describe("trying the connection", () => {
    it("asks for one word and keeps the connection's result in the link", async () => {
        const endpoint = await openaiFake();
        const home = await testHome(scratch(), {
            env: {
                HAMLET_MODEL: "local::org/large",
                HAMLET_BASE_URL: endpoint.url,
            },
        });
        expect(await checkModel(home, undefined, context)).toBe("Hello.");
        expect(JSON.stringify(endpoint.requests[0])).toContain(
            "Say hello in one word.",
        );
        expect(await checkConnection(home, context)).toBe(
            "Connected, 1 model(s)",
        );
        expect(await home.harness.snapshot(SettingsDoc, context)).toMatchObject(
            { link: { state: "connected", models: 1 } },
        );
        await expect(checkModel(home, "missing", context)).rejects.toThrow(
            "`missing` names no endpoint",
        );
        await expect(checkModel(home, "far::small", context)).rejects.toThrow(
            "the settings do not have",
        );
    });

    it("gives up after 90 seconds", async () => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), { models });
        faux.setResponses([async () => new Promise(() => {})]);
        vi.useFakeTimers();
        try {
            const checking = checkModel(home, undefined, context);
            await vi.advanceTimersByTimeAsync(90_000);
            expect(await checking).toBe(
                "It took too long to answer. Try again, or pick a smaller model.",
            );
            expect(
                await home.harness.snapshot(SettingsDoc, context),
            ).toMatchObject({ link: { state: "failed" } });
        } finally {
            vi.useRealTimers();
        }
    });
});

it("tells a refused key from an unreachable address and an empty account", () => {
    expect(plainly("HTTP 401")).toBe(
        "That key was not accepted. Check that you copied all of it and that it is still active.",
    );
    expect(plainly("HTTP 400: Please pass a valid API key")).toContain(
        "key was not accepted",
    );
    expect(plainly("could not reach it")).toBe(
        "Could not reach it. Check your connection and the address.",
    );
    expect(plainly("HTTP 402")).toContain("no credit left");
    expect(plainly("HTTP 404")).toContain("usually ends in /v1");
});

it.each([
    [
        "HTTP 404: model missing",
        true,
        "That model did not answer a hello. Pick another one. (HTTP 404: model missing)",
    ],
    [
        "max_tokens must be at most 4010",
        false,
        "It answered with an error. Try again. (max_tokens must be at most 4010)",
    ],
    [
        "HTTP 500: request req_8401a failed",
        false,
        "It answered with an error. Try again. (HTTP 500: request req_8401a failed)",
    ],
    [
        "fetch failed",
        false,
        "It answered with an error. Try again. (fetch failed)",
    ],
    ["timeout", false, "It answered with an error. Try again. (timeout)"],
    [
        "HTTP 403",
        false,
        "That key was not accepted. Check that you copied all of it and that it is still active.",
    ],
] as const)(
    "uses Rust's plain rule for %s (hello=%s)",
    (error, hello, expected) => {
        expect(plainly(error, hello)).toBe(expected);
    },
);

it("scrubs the whole of overlapping credentials before keeping a failed hello", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    await home.credentials.modify("first", async () => ({
        type: "api_key",
        key: "sk-or-v1",
    }));
    await home.credentials.modify("second", async () => ({
        type: "api_key",
        key: "sk-or-v1-0123456789abcdef",
    }));
    const { fauxAssistantMessage } = await import(
        "@earendil-works/pi-ai/providers/faux"
    );
    faux.setResponses([
        fauxAssistantMessage("", {
            stopReason: "error",
            errorMessage: "key sk-or-v1-0123456789abcdef",
        }),
    ]);
    const expected =
        "That model did not answer a hello. Pick another one. (Error: key [REDACTED])";
    expect(await checkModel(home, undefined, context)).toBe(expected);
    expect(
        (await home.harness.snapshot(SettingsDoc, context))?.link.words,
    ).toBe(expected);
});

it.each([
    [
        401,
        "That key was not accepted. Check that you copied all of it and that it is still active.",
    ],
    [
        403,
        "That key was not accepted. Check that you copied all of it and that it is still active.",
    ],
    [
        402,
        "Your provider account has no credit left. Add some, then try again.",
    ],
])(
    "names a hello refused by the endpoint with HTTP %i",
    async (status, words) => {
        const { createServer } = await import("node:http");
        const { closeAfter } = await import("./support.ts");
        const endpoint = createServer((request, response) => {
            response.setHeader("Content-Type", "application/json");
            if (request.url === "/v1/models") {
                response.end(JSON.stringify({ data: [{ id: "small" }] }));
            } else {
                response.writeHead(status);
                response.end(JSON.stringify({ message: "nope" }));
            }
        });
        await new Promise<void>((resolve) =>
            endpoint.listen(0, "127.0.0.1", resolve),
        );
        closeAfter(() => {
            endpoint.closeAllConnections();
            endpoint.close();
        });
        const address = endpoint.address();
        if (address === null || typeof address === "string")
            throw new Error("No endpoint");
        const home = await testHome(scratch(), {
            env: {
                HAMLET_MODEL: "local::small",
                HAMLET_BASE_URL: `http://127.0.0.1:${address.port}/v1`,
            },
        });
        expect(await checkModel(home, undefined, context)).toBe(words);
        expect(
            (await home.harness.snapshot(SettingsDoc, context))?.link.words,
        ).toBe(words);
    },
);
