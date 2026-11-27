import assert from "node:assert/strict";
import { test } from "node:test";
import {
    createFauxCore,
    createProvider,
    fauxAssistantMessage,
    type MutableModels,
} from "@earendil-works/pi-ai";
import { keysOf } from "../src/agent/credentials.ts";
import { signIns } from "../src/agent/login.ts";
import type { ChatPage } from "../src/agent/messages/agent.ts";
import type { Kept } from "../src/agent/messages/realm.ts";
import { checkModel } from "../src/agent/models.ts";
import {
    credentialsFile,
    dumped,
    from,
    named,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import {
    assertNowhere,
    client,
    forms,
    keySignIn,
    loopback,
    secret,
    subscription,
} from "./login-testing.ts";

async function say(agent: Woken, text: string): Promise<void> {
    await agent.actions.submit(agent.by(from.realm), {
        id: `line-${text}`,
        input: { type: "line", text },
    });
    await agent.idle();
}

async function page(agent: Woken): Promise<ChatPage> {
    return agent.actions.read_chat(agent.by(from.client), {
        id: "00000000-0000-4000-8000-0000000000ff",
        chat: { with: 7 },
    });
}

test("a key sign-in leaves the key only in credentials.json", async (t) => {
    const key = secret(t, "key");
    const { file, calls, sent } = credentialsFile();
    const keysSeen: (string | undefined)[] = [];
    const agent = await wakeAgent(t, {
        calls,
        models: loopback((_transcript, options) => {
            keysSeen.push(options?.apiKey);
            return fauxAssistantMessage(`My key is ${options?.apiKey}.`);
        }),
    });
    const hand = client(agent, signIns(agent.config.models));

    const first = await hand.start("loopback", "key");
    assert.deepEqual(first, {
        type: "step",
        login: first.login,
        step: { prompt: "Paste your key.", secret: true },
    });
    const done = await hand.reply(first.login, key);
    assert.deepEqual(done, { type: "done", login: first.login });
    assert.deepEqual(file.get("loopback"), { type: "key", key });

    await agent.actions.copies(agent.by(from.realm), {
        model: { version: 1, value: { model: "loopback::large", fast: false } },
    });
    await say(agent, "What is your key?");
    assert.deepEqual(keysSeen, [key], "the request carried the key");
    const answer = (await page(agent)).items.find(
        (item) => item.kind === "assistant",
    );
    assert.equal(answer?.text, "My key is [REDACTED].");

    assert.deepEqual(
        sent
            .filter((call) =>
                forms(key).some((form) =>
                    String(JSON.stringify(call.message)).includes(form),
                ),
            )
            .map((call) => call.name),
        ["keep_credential"],
        "only the realm's keep was given the key",
    );
    assertNowhere([key], {
        database: await dumped(agent),
        log: agent.reported,
        events: agent.events,
        "sign-in answers": hand.answers,
        "chat page": await page(agent),
    });
    await agent.stop();
});

test("an OAuth credential is refreshed once and kept", async (t) => {
    const fixture = subscription(t);
    const other = secret(t, "other-provider-key");
    const { file, calls, sent } = credentialsFile();
    file.set("other", { type: "key", key: other });
    file.set("chatgpt", {
        type: "oauth",
        login: {
            ...fixture.before,
            // Long past, so the first request refreshes it.
            expires: 1,
            extra: JSON.stringify({ clientId: "stable-installation" }),
        },
    });
    const agent = await wakeAgent(t, { calls, models: fixture.listing });
    await agent.actions.copies(agent.by(from.realm), {
        model: {
            version: 1,
            value: { model: "chatgpt::offline", fast: false },
        },
    });
    const realm = {
        credential: async ({ provider }: { provider: string }) => {
            const held = file.get(provider);
            assert.ok(held !== undefined);
            return held;
        },
    };
    const keep = async (message: { provider: string; value: Kept }) => {
        file.set(message.provider, message.value);
        return null;
    };

    const line = agent.actions.submit(agent.by(from.realm), {
        id: "line-1",
        input: { type: "line", text: "Echo your tokens." },
    });
    await until(() => fixture.refreshes === 1);
    const checked = checkModel(
        agent.config.models,
        keysOf(realm, () => {}, { listed: agent.config.models, keep }),
        "chatgpt::offline",
    );
    // A stopped turn leaves the rotation it started to finish.
    await line;
    await agent.actions.stop(agent.by(from.realm), { number: 1 });
    fixture.refreshHeld.resolve();
    const words = (await checked).words;
    await agent.idle();
    await say(agent, "Echo them again.");

    assert.equal(fixture.refreshes, 1, "one refresh for both requests");
    assert.ok(fixture.keysSeen.length >= 2);
    assert.ok(
        fixture.keysSeen.every((key) => key === fixture.after.access),
        "every request carried the rotated token",
    );
    const stored = file.get("chatgpt");
    assert.equal(stored?.type, "oauth");
    assert.equal(stored.login.access, fixture.after.access);
    assert.equal(stored.login.refresh, fixture.after.refresh);
    assert.ok(stored.login.expires > 1);
    assert.deepEqual(JSON.parse(stored.login.extra ?? "{}"), {
        clientId: "stable-installation",
    });
    assert.deepEqual(file.get("other"), { type: "key", key: other });
    assert.deepEqual(
        named(sent, "turn_failed"),
        [],
        "the stopped turn did not fail",
    );
    assert.equal(
        named(sent, "keep_credential").length,
        1,
        "the rotation was kept once, by the agent that refreshed",
    );
    assert.equal(
        words,
        "OAuth echoed [REDACTED] [REDACTED] [REDACTED] [REDACTED]",
    );
    assert.deepEqual(
        (await page(agent)).items.filter((item) => item.kind === "error"),
        [],
    );
    const shown = (await page(agent)).items.filter(
        (item) => item.kind === "assistant",
    );
    assert.ok(shown.length > 0);
    for (const item of shown) {
        assert.equal(
            item.text,
            "OAuth echoed [REDACTED] [REDACTED] [REDACTED] [REDACTED]",
        );
    }
    assertNowhere(
        [...Object.values(fixture.before), ...Object.values(fixture.after)],
        {
            database: await dumped(agent),
            log: agent.reported,
            events: agent.events,
            "chat page": await page(agent),
            "calls but the keep": sent
                .filter((call) => call.name !== "keep_credential")
                .map((call) => call.message),
        },
    );
    await agent.stop();
});

test("every typed answer to a sign-in joins the scrub list, as today", async (t) => {
    const key = secret(t, "key");
    /** Made up, and scrubbed from the host once typed. */
    const profile = "profile-made-up-41c9";
    const { calls } = credentialsFile();
    const core = createFauxCore({
        provider: "profiled",
        models: [{ id: "large", name: "Large", contextWindow: 64_000 }],
    });
    core.setResponses([
        () => fauxAssistantMessage(`Profile ${profile}, key ${key}.`),
    ]);
    const provider = createProvider({
        id: "profiled",
        models: core.models,
        api: { stream: core.stream, streamSimple: core.streamSimple },
        auth: {
            apiKey: {
                name: "profiled key",
                // As Bedrock's key sign-in asks for a profile's name.
                login: async (interaction) => {
                    await interaction.prompt({
                        type: "text",
                        message: "Enter AWS profile name",
                    });
                    return {
                        type: "api_key",
                        key: await interaction.prompt({
                            type: "secret",
                            message: "Paste your key.",
                        }),
                    };
                },
                resolve: async ({ credential }) =>
                    credential?.key === undefined
                        ? undefined
                        : { auth: { apiKey: credential.key } },
            },
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        models: (listed) => {
            (listed as MutableModels).setProvider(provider);
            return listed;
        },
    });
    const hand = client(agent, signIns(agent.config.models));
    const login = (await hand.start("profiled", "key")).login;
    assert.deepEqual(await hand.reply(login, profile), {
        type: "step",
        login,
        step: { prompt: "Paste your key.", secret: true },
    });
    assert.deepEqual(await hand.reply(login, key), { type: "done", login });
    await agent.actions.copies(agent.by(from.realm), {
        model: {
            version: 1,
            value: { model: "profiled::large", fast: false },
        },
    });

    await say(agent, "Which profile?");

    const answer = (await page(agent)).items.find(
        (item) => item.kind === "assistant",
    );
    assert.equal(answer?.text, "Profile [REDACTED], key [REDACTED].");
    await agent.stop();
});

test("a secret signed in after it was said reaches no client", async (t) => {
    const key = secret(t, "key");
    const { calls } = credentialsFile();
    const agent = await wakeAgent(t, {
        calls,
        models: loopback(() => fauxAssistantMessage("Hello.")),
    });
    await say(agent, `My key is ${key}.`);
    const hand = client(agent, signIns(agent.config.models));
    const login = (await hand.start("loopback", "key")).login;
    await hand.reply(login, key);
    const seen = agent.events.length;

    await say(agent, "Hello again.");

    const line = (await page(agent)).items.find((item) => item.kind === "user");
    assert.equal(line?.text, "My key is [REDACTED].");
    const history = await agent.actions.read_history(agent.by(from.client));
    const dump = await dumped(agent);
    assert.ok(
        forms(key).some((form) => dump.includes(form)),
        "the search finds the line as the realm gave it, which the agent keeps",
    );
    assertNowhere([key], {
        "chat page": await page(agent),
        "events after the sign-in": agent.events.slice(seen),
        history,
    });
    await agent.stop();
});

test("a step quoting a typed secret reaches the client scrubbed", async (t) => {
    const key = secret(t, "key");
    const { calls } = credentialsFile();
    const agent = await wakeAgent(t, {
        calls,
        models: keySignIn("echoing", async (interaction) => {
            const typed = await interaction.prompt({
                type: "secret",
                message: "Paste your key.",
            });
            interaction.notify({
                type: "progress",
                message: `Checking ${typed}`,
            });
            return {
                type: "api_key",
                key: await interaction.prompt({
                    type: "secret",
                    message: `The key ${typed} was not accepted. Paste it again.`,
                }),
            };
        }),
    });
    const limit = Promise.withResolvers<void>();
    const hand = client(
        agent,
        signIns(agent.config.models, { waited: () => limit.promise }),
    );
    const login = (await hand.start("echoing", "key")).login;
    const asked = {
        prompt: "The key [REDACTED] was not accepted. Paste it again.",
        secret: true,
    };

    assert.deepEqual(await hand.reply(login, key), {
        type: "step",
        login,
        step: { progress: "Checking [REDACTED]" },
    });
    assert.deepEqual(await hand.next(login), {
        type: "step",
        login,
        step: asked,
    });
    const again = hand.next(login);
    limit.resolve();
    assert.deepEqual(await again, { type: "step", login, step: asked });
    await hand.cancel(login);
    await agent.stop();
});

test("after a wake, a stored key is scrubbed from what a client reads, and no key is asked for", async (t) => {
    const key = secret(t, "earlier-life-key");
    const { file, calls, sent } = credentialsFile();
    file.set("loopback", { type: "key", key });
    const agent = await wakeAgent(t, {
        calls,
        models: loopback(() => fauxAssistantMessage("Hello.")),
    });
    // Said to the faux model, so no model call asks for the key.
    await say(agent, `My key is ${key}.`);
    await agent.actions.copies(agent.by(from.realm), {
        model: { version: 1, value: { model: "loopback::large", fast: false } },
    });
    await agent.stop();

    const woken = await agent.again();
    const line = async () =>
        (await page(woken)).items.find((item) => item.kind === "user")?.text;
    // The host loaded the list before this test, so a read waits for no
    // later wake's refresh.
    await until(async () => (await line()) === "My key is [REDACTED].").catch(
        () => {},
    );

    assert.equal(await line(), "My key is [REDACTED].");
    assertNowhere([key], {
        "chat page": await page(woken),
        history: await woken.actions.read_history(woken.by(from.client)),
    });
    assert.deepEqual(named(sent, "credential"), []);
    assert.deepEqual(woken.reported, []);
    await woken.stop();
});
