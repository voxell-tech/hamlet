import { PassThrough } from "node:stream";
import { expect, test } from "vitest";
import { Channel, type Outgoing } from "../src/channel.ts";
import { serverCredentials } from "../src/credentials.ts";
import { signIn } from "../src/sign-in.ts";

test("a cancelled callback prompt cannot answer its replacement or cancel the login", async () => {
    const input = new PassThrough();
    const output = new PassThrough();
    const sent: Array<Extract<Outgoing, { type: "provider_event" }>> = [];
    output.on("data", (bytes: Buffer) =>
        sent.push(JSON.parse(bytes.toString())),
    );
    const session = signIn(new Channel(input, output), "login");
    const callback = new AbortController();
    const old = session.interaction.prompt({
        type: "manual_code",
        message: "Callback",
        signal: callback.signal,
    });
    const oldPrompt = sent.at(-1)?.prompt;
    if (oldPrompt === undefined) throw new Error("Missing callback prompt");
    callback.abort();
    await expect(old).rejects.toThrow("cancelled");
    expect(session.interaction.signal?.aborted).toBe(false);
    const current = session.interaction.prompt({
        type: "select",
        message: "Account",
        options: [
            { id: "work", label: "Work" },
            { id: "personal", label: "Personal" },
        ],
    });
    const currentPrompt = sent.at(-1)?.prompt;
    if (currentPrompt === undefined) throw new Error("Missing account prompt");
    expect(sent.at(-1)?.step).toEqual({
        prompt: "Account\nwork: Work\npersonal: Personal",
        secret: false,
    });
    session.control({
        v: 1,
        id: "late",
        type: "provider_control",
        scope: "login",
        prompt: oldPrompt,
        value: "old-code",
        cancel: false,
    });
    session.control({
        v: 1,
        id: "current",
        type: "provider_control",
        scope: "login",
        prompt: currentPrompt,
        value: "work",
        cancel: false,
    });
    await expect(current).resolves.toBe("work");
    session.stop();
    await expect(
        session.interaction.prompt({ type: "text", message: "After cancel" }),
    ).rejects.toThrow("cancelled");
    input.end();
    output.end();
});

test("cancellation skips a queued credential callback but a started rotation waits for persistence", async () => {
    const input = new PassThrough();
    const output = new PassThrough();
    const requests: Array<
        Extract<Outgoing, { type: "credential_request" }> & { id: string }
    > = [];
    output.on("data", (bytes: Buffer) =>
        requests.push(JSON.parse(bytes.toString())),
    );
    const jobs = new Set<Promise<unknown>>();
    const store = serverCredentials(
        new Channel(input, output),
        "attempt",
        "chatgpt",
        jobs,
    );
    const respond = (fields: object) => {
        input.write(
            `${JSON.stringify({ v: 1, id: "host", type: "credential_result", re: requests.at(-1)?.id, ...fields })}\n`,
        );
    };
    const queued = new AbortController();
    let callbacks = 0;
    const first = store.modify(
        "chatgpt",
        async () => {
            callbacks++;
            return undefined;
        },
        { signal: queued.signal },
    );
    queued.abort(new Error("queued cancellation"));
    respond({ lease: "queued" });
    await new Promise<void>((resolve) => setImmediate(resolve));
    respond({});
    await expect(first).rejects.toThrow("queued cancellation");
    expect(callbacks).toBe(0);
    const active = new AbortController();
    const second = store.modify(
        "chatgpt",
        async () => {
            active.abort();
            return {
                type: "oauth",
                access: "offline-access",
                refresh: "offline-refresh",
                expires: 123,
                clientId: "stable",
                scopes: ["profile"],
            };
        },
        { signal: active.signal },
    );
    respond({ lease: "active" });
    await new Promise<void>((resolve) => setImmediate(resolve));
    let resolved = false;
    void second.then(() => {
        resolved = true;
    });
    await Promise.resolve();
    expect(resolved).toBe(false);
    respond({
        credential: {
            type: "oauth",
            access: "offline-access",
            refresh: "offline-refresh",
            expires: 123,
            extra: { clientId: "stable", scopes: ["profile"] },
        },
    });
    await expect(second).resolves.toEqual({
        type: "oauth",
        access: "offline-access",
        refresh: "offline-refresh",
        expires: 123,
        clientId: "stable",
        scopes: ["profile"],
    });
    const failed = store.modify("chatgpt", async () => ({
        type: "api_key",
        key: "unsaved",
    }));
    respond({ lease: "failed" });
    await new Promise<void>((resolve) => setImmediate(resolve));
    respond({ error: "disk full" });
    await expect(failed).rejects.toThrow("disk full");
    expect(jobs.size).toBe(0);
    await expect(store.read("other-provider")).rejects.toThrow("one provider");
    input.end();
    output.end();
});
