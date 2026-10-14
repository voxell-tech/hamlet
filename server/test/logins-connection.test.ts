import { randomUUID } from "node:crypto";
import type { OAuthAuth } from "@earendil-works/pi-ai";
import { expect, it, vi } from "vitest";
import type { LoginProgress } from "../src/protocol.ts";
import {
    changes,
    joined,
    received,
    request,
    served,
} from "./connection-support.ts";
import { openaiFake } from "./openai-fake.ts";

it("streams a key login and provider changes without ever sending the key to any client (S7)", async () => {
    const endpoint = await openaiFake();
    const run = await served({
        env: {
            HAMLET_MODEL: "local::org/large",
            HAMLET_BASE_URL: endpoint.url,
        },
    });
    const id = randomUUID();
    const first = await request(run.connection, run.messages, {
        type: "login",
        id,
        start: { provider: "local", method: "key" },
    });
    expect(first.type).toBe("login");
    const prompt = first as LoginProgress;
    expect(prompt.step).toEqual({ prompt: "Paste your key.", secret: true });
    const key = 'secret-with-"quote';
    run.connection.send({
        type: "login",
        id: randomUUID(),
        reply: { login: prompt.login, value: key },
    });
    await received(
        run.messages,
        (message): message is LoginProgress =>
            message.type === "login" && "done" in message.step,
    );
    expect(await run.home.credentials.read("local")).toMatchObject({
        type: "api_key",
        key,
    });
    expect(JSON.stringify(run.messages)).not.toContain("secret-with-");
    expect(changes(run.messages)).toContainEqual(
        expect.objectContaining({
            type: "settings",
            settings: expect.objectContaining({
                endpoints: [
                    expect.objectContaining({ name: "local", key: true }),
                ],
            }),
        }),
    );
    const openai = await request(run.connection, run.messages, {
        type: "login",
        id: randomUUID(),
        start: { provider: "openai", method: "key" },
    });
    if (openai.type !== "login") throw new Error("No key prompt");
    run.connection.send({
        type: "login",
        id: randomUUID(),
        reply: { login: openai.login, value: "openai-secret" },
    });
    await received(
        run.messages,
        (message): message is LoginProgress =>
            message.type === "login" &&
            "done" in message.step &&
            message.step.done === "openai",
    );
    expect(changes(run.messages)).toContainEqual(
        expect.objectContaining({
            type: "providers",
            providers: expect.arrayContaining([
                expect.objectContaining({ id: "openai", configured: true }),
            ]),
        }),
    );
    expect(JSON.stringify(run.messages)).not.toContain("openai-secret");
});

it("streams sign-in URLs, codes and progress, scrubs a failed step, and binds every login reply to its connection", async () => {
    const endpoint = await openaiFake();
    const oauth: OAuthAuth = {
        name: "ChatGPT",
        isSubscription: true,
        async login(interaction) {
            interaction.notify({
                type: "auth_url",
                url: "https://example.invalid/login",
                instructions: "Open this.",
            });
            interaction.notify({
                type: "device_code",
                verificationUri: "https://example.invalid/code",
                userCode: "ABCD",
            });
            const value = await interaction.prompt({
                type: "secret",
                message: "Paste a code.",
            });
            interaction.notify({
                type: "progress",
                message: `Checking ${value}`,
            });
            throw new Error(`Rejected ${value}`);
        },
        refresh: async (credential) => credential,
        toAuth: async (credential) => ({ apiKey: credential.access }),
    };
    const run = await served({
        logins: { chatgpt: oauth },
        env: {
            HAMLET_MODEL: "local::org/large",
            HAMLET_BASE_URL: endpoint.url,
        },
    });
    const id = randomUUID();
    run.connection.send({
        type: "login",
        id,
        start: { provider: "chatgpt", method: "subscription" },
    });
    const prompt = await received(
        run.messages,
        (message): message is LoginProgress =>
            message.type === "login" && "prompt" in message.step,
    );
    expect(
        run.messages
            .filter((message) => message.type === "login")
            .map((message) => message.step),
    ).toContainEqual({ code: "ABCD" });
    const other = await joined(run.server.url, run.home.credentials.token);
    expect(
        await request(other.connection, other.messages, {
            type: "login",
            id: randomUUID(),
            reply: { login: prompt.login, value: "not-yours" },
        }),
    ).toMatchObject({
        kind: "error",
        text: "There is no sign-in here with that id.",
    });
    run.connection.send({
        type: "login",
        id: randomUUID(),
        reply: { login: prompt.login, value: "not-stored-secret" },
    });
    const failed = await received(
        run.messages,
        (message): message is LoginProgress =>
            message.type === "login" && "failed" in message.step,
    );
    expect(failed.step).toEqual({ failed: "Rejected [REDACTED]" });
    expect(JSON.stringify(run.messages)).not.toContain("not-stored-secret");
    const second = randomUUID();
    run.connection.send({
        type: "login",
        id: second,
        start: { provider: "chatgpt", method: "subscription" },
    });
    const waiting = await received(
        run.messages,
        (message): message is LoginProgress =>
            message.type === "login" &&
            message.to === second &&
            "prompt" in message.step,
    );
    expect(
        await request(run.connection, run.messages, {
            type: "login",
            id: randomUUID(),
            cancel: waiting.login,
        }),
    ).toMatchObject({ type: "reply", kind: "notice" });
    await vi.waitFor(() =>
        expect(
            run.messages.filter(
                (message) =>
                    message.type === "login" && "failed" in message.step,
            ),
        ).toHaveLength(2),
    );
});
