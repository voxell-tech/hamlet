import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import {
    createFauxCore,
    createProvider,
    type FauxResponseFactory,
    fauxAssistantMessage,
    type Models,
    type MutableModels,
} from "@earendil-works/pi-ai";
import { chatActions } from "../src/agent/chat.ts";
import { type SignIns, signIns } from "../src/agent/login.ts";
import type {
    ChatPage,
    Kept,
    Login,
    LoginAnswer,
} from "../src/agent/messages.ts";
import { checkModel, keysOf } from "../src/agent/models.ts";
import {
    fakeCalls,
    from,
    named,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";

/**
 * A made-up secret named for the test, so no test passes on a secret an
 * earlier one left in the host's memory. It holds a double quote, as A13's
 * key does, so a search must look for its escaped form too.
 */
function secret(t: TestContext, what: string): string {
    return `made-up-${what}-"${t.name.replace(/\W+/g, "-")}`;
}

/**
 * Every form a secret takes in text: as it is, escaped in JSON, and escaped
 * twice, as in a row's JSON column dumped as JSON.
 */
function forms(text: string): string[] {
    const once = JSON.stringify(text).slice(1, -1);
    return [text, once, JSON.stringify(once).slice(1, -1)];
}

function assertNowhere(
    secrets: readonly string[],
    where: Record<string, unknown>,
): void {
    for (const [what, seen] of Object.entries(where)) {
        const text = typeof seen === "string" ? seen : JSON.stringify(seen);
        for (const found of secrets.flatMap(forms)) {
            assert.ok(!text.includes(found), `the ${what} hold ${found}`);
        }
    }
}

/** The realm's half: `credentials.json` as the calls see it. */
function credentialsFile() {
    const file = new Map<string, Kept>();
    const { calls, sent } = fakeCalls({
        workspace: { cancel: () => ({ commands: [] }) },
        realm: {
            credential: ({ provider }) =>
                file.get(provider) ?? {
                    type: "refused",
                    why: { type: "not_signed_in", provider },
                    words: `Sign in to ${provider} first.`,
                },
            keep_credential: ({ provider, value }) => {
                file.set(provider, value);
                return null;
            },
        },
    });
    return { file, calls, sent };
}

/**
 * The provider `loopback`, whose key sign-in asks as today's endpoint's
 * does (`harnesses/pi/src/providers.ts`, `compatible`), and whose one model
 * answers with `answer`.
 */
function loopback(answer: FauxResponseFactory) {
    const core = createFauxCore({
        provider: "loopback",
        models: [{ id: "large", name: "Large", contextWindow: 64_000 }],
    });
    core.setResponses(Array.from({ length: 20 }, () => answer));
    const provider = createProvider({
        id: "loopback",
        models: core.models,
        api: { stream: core.stream, streamSimple: core.streamSimple },
        auth: {
            apiKey: {
                name: "loopback key",
                login: async (interaction) => ({
                    type: "api_key",
                    key: (
                        await interaction.prompt({
                            type: "secret",
                            message: "Paste your key.",
                        })
                    ).trim(),
                }),
                resolve: async ({ credential }) =>
                    credential?.key === undefined
                        ? undefined
                        : { auth: { apiKey: credential.key } },
            },
        },
    });
    return (listed: Models): Models => {
        (listed as MutableModels).setProvider(provider);
        return listed;
    };
}

let requests = 0;

/** A client's `login` calls of the Hand, with every answer kept. */
function client(agent: Woken, given: SignIns) {
    const login = chatActions({ ...agent.config, signIns: given }).login;
    const answers: LoginAnswer[] = [];
    let last: Login | undefined;
    const send = async (message: Login): Promise<LoginAnswer> => {
        last = message;
        const answer = await login(agent.by(from.client), message);
        answers.push(answer);
        return answer;
    };
    const call = (form: Omit<Login, "id">): Promise<LoginAnswer> => {
        requests += 1;
        const id = `00000000-0000-4000-8000-${String(requests).padStart(12, "0")}`;
        return send({ id, ...form } as Login);
    };
    return {
        answers,
        start: (provider: string, method: "key" | "subscription") =>
            call({ start: { provider, method } }),
        reply: (login: string, value: string) =>
            call({ reply: { login, value } }),
        /** Asks for the next step of a sign-in that asks for nothing. */
        next: (login: string) => call({ reply: { login, value: "" } }),
        cancel: (login: string) => call({ cancel: login }),
        /** The last call again, under its request id. */
        again: () => {
            if (last === undefined) throw new Error("nothing was called");
            return send(last);
        },
    };
}

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

async function dumped(agent: Woken): Promise<string> {
    const tables = await agent.c.db.all<{ name: string }>(
        "SELECT name FROM sqlite_master WHERE type = 'table'",
    );
    const decoder = new TextDecoder();
    let text = "";
    for (const { name } of tables) {
        const rows = await agent.c.db.all<Record<string, unknown>>(
            `SELECT * FROM "${name}"`,
        );
        text += JSON.stringify(rows, (_key, value: unknown) =>
            value instanceof Uint8Array ? decoder.decode(value) : value,
        );
    }
    return text;
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

test("A13: a key sign-in's refusals and its repeats are today's", async (t) => {
    const key = secret(t, "key");
    const { file, calls, sent } = credentialsFile();
    const agent = await wakeAgent(t, {
        calls,
        models: loopback(() => fauxAssistantMessage("Hello.")),
    });
    const hand = client(agent, signIns(agent.config.models));
    const failedWith = (answer: LoginAnswer) =>
        answer.type === "failed" ? answer.words : answer;

    assert.equal(
        failedWith(await hand.start("nowhere", "key")),
        "Unknown provider: nowhere",
    );
    assert.equal(
        failedWith(await hand.start("loopback", "subscription")),
        "Signing in with loopback is not offered. Use an API key.",
    );
    assert.equal(
        failedWith(await hand.reply("no-such-login", key)),
        "No sign-in of yours is open.",
    );

    const cancelled = await hand.start("loopback", "key");
    assert.equal(
        failedWith(await hand.cancel(cancelled.login)),
        "Sign-in cancelled.",
    );
    assert.equal(
        failedWith(await hand.reply(cancelled.login, key)),
        "No sign-in of yours is open.",
    );

    const started = await hand.start("loopback", "key");
    assert.deepEqual(await hand.again(), started, "a repeated start");
    const done = await hand.reply(started.login, key);
    assert.deepEqual(done, { type: "done", login: started.login });
    assert.deepEqual(await hand.again(), done, "a repeated reply");
    assert.equal(
        sent.filter((call) => call.name === "keep_credential").length,
        1,
        "the key was kept once",
    );
    assert.deepEqual([...file.keys()], ["loopback"]);
    await agent.stop();
});

/** A loopback subscription's sign-in, its refresh, and what they did. */
interface Fixture {
    readonly listing: (listed: Models) => Models;
    /** The code the sign-in takes. */
    readonly code: string;
    readonly before: { readonly access: string; readonly refresh: string };
    readonly after: { readonly access: string; readonly refresh: string };
    /** Sign-ins begun, and those cancelled before they ended. */
    begun: number;
    dropped: number;
    refreshes: number;
    /** What each request to the model carried as its key. */
    readonly keysSeen: (string | undefined)[];
    /** Held until a test lets the refresh answer. */
    readonly refreshHeld: PromiseWithResolvers<void>;
    /** The sign-in asks for its code once this resolves, as a slow provider would. */
    beforePrompt: Promise<void>;
}

/**
 * The provider `chatgpt` on loopback, after today's OAuth fixture
 * (`harnesses/pi/test/fixtures/oauth.ts`): its sign-in shows a link, a
 * device code and a progress line, then asks for the code. Its model
 * echoes every token it knows, as A19's does.
 */
function subscription(t: TestContext): Fixture {
    const fixture: Fixture = {
        listing: (listed) => listed,
        code: secret(t, "code"),
        before: {
            access: secret(t, "access-before"),
            refresh: secret(t, "refresh-before"),
        },
        after: {
            access: secret(t, "access-after"),
            refresh: secret(t, "refresh-after"),
        },
        begun: 0,
        dropped: 0,
        refreshes: 0,
        keysSeen: [],
        refreshHeld: Promise.withResolvers<void>(),
        beforePrompt: Promise.resolve(),
    };
    const core = createFauxCore({
        provider: "chatgpt",
        models: [{ id: "offline", name: "Offline", contextWindow: 64_000 }],
    });
    const echo: FauxResponseFactory = (_transcript, options) => {
        fixture.keysSeen.push(options?.apiKey);
        const { before, after } = fixture;
        return fauxAssistantMessage(
            `OAuth echoed ${before.access} ${before.refresh} ${after.access} ${after.refresh}`,
        );
    };
    core.setResponses(Array.from({ length: 20 }, () => echo));
    const provider = createProvider({
        id: "chatgpt",
        models: core.models,
        api: { stream: core.stream, streamSimple: core.streamSimple },
        auth: {
            oauth: {
                name: "loopback subscription",
                login: async (interaction, options) => {
                    fixture.begun += 1;
                    interaction.signal.addEventListener("abort", () => {
                        fixture.dropped += 1;
                    });
                    interaction.notify({
                        type: "device_code",
                        verificationUri: "https://example.invalid/oauth",
                        userCode: "SAFE-CODE",
                    });
                    interaction.notify({
                        type: "progress",
                        message: "Waiting for the offline code",
                    });
                    await fixture.beforePrompt;
                    const code = await interaction.prompt({
                        type: "manual_code",
                        message: "Paste the code.",
                    });
                    if (code !== fixture.code) {
                        throw new Error(`The code ${code} was rejected.`);
                    }
                    return {
                        type: "oauth",
                        ...fixture.before,
                        expires: Date.now() + 3_600_000,
                        clientId: options?.getDeviceId?.(),
                        agent: options?.agentName,
                        scopes: ["offline", "profile"],
                    };
                },
                refresh: async (credential) => {
                    fixture.refreshes += 1;
                    await fixture.refreshHeld.promise;
                    return {
                        ...credential,
                        ...fixture.after,
                        expires: Date.now() + 3_600_000,
                    };
                },
                toAuth: async (credential) => ({ apiKey: credential.access }),
            },
        },
    });
    return Object.assign(fixture, {
        listing: (listed: Models): Models => {
            (listed as MutableModels).setProvider(provider);
            return listed;
        },
    });
}

test("A17: an OAuth sign-in steps through its link, code and prompt", async (t) => {
    const fixture = subscription(t);
    const { file, calls } = credentialsFile();
    const agent = await wakeAgent(t, { calls, models: fixture.listing });
    const hand = client(
        agent,
        signIns(agent.config.models, { deviceId: "stable-installation" }),
    );

    assert.deepEqual(
        await hand.start("chatgpt", "key"),
        {
            type: "failed",
            login: hand.answers.at(-1)?.login,
            words: "Signing in with chatgpt is not offered. Use an API key.",
        },
        "chatgpt offers only a subscription",
    );
    const login = (await hand.start("chatgpt", "subscription")).login;
    const steps = [hand.answers.at(-1)];
    for (let i = 0; i < 3; i += 1) steps.push(await hand.next(login));
    assert.deepEqual(
        steps.map((answer) => (answer?.type === "step" ? answer.step : answer)),
        [
            { url: "https://example.invalid/oauth" },
            { code: "SAFE-CODE" },
            { progress: "Waiting for the offline code" },
            { prompt: "Paste the code.", secret: true },
        ],
    );
    const rejected = secret(t, "rejected-code");
    const failed = await hand.reply(login, rejected);
    assert.deepEqual(failed, {
        type: "failed",
        login,
        words: "Error: The code [REDACTED] was rejected.",
    });
    assert.equal(file.get("chatgpt"), undefined, "nothing was kept");

    const cancelled = (await hand.start("chatgpt", "subscription")).login;
    assert.deepEqual(await hand.cancel(cancelled), {
        type: "failed",
        login: cancelled,
        words: "Sign-in cancelled.",
    });
    assert.equal(fixture.dropped, 1, "the cancel ended the provider's flow");

    const kept = (await hand.start("chatgpt", "subscription")).login;
    for (let i = 0; i < 3; i += 1) await hand.next(kept);
    assert.deepEqual(await hand.reply(kept, fixture.code), {
        type: "done",
        login: kept,
    });
    const stored = file.get("chatgpt");
    assert.equal(stored?.type, "oauth");
    assert.deepEqual(stored.login.access, fixture.before.access);
    assert.deepEqual(JSON.parse(stored.login.extra ?? "{}"), {
        clientId: "stable-installation",
        agent: "Hamlet",
        scopes: ["offline", "profile"],
    });
    assertNowhere([rejected, fixture.code, ...Object.values(fixture.before)], {
        "sign-in answers": hand.answers,
        log: agent.reported,
        events: agent.events,
        database: await dumped(agent),
    });
    await agent.stop();
});

test("an OAuth sign-in dropped halfway starts again", async (t) => {
    const fixture = subscription(t);
    const { file, calls, sent } = credentialsFile();
    const agent = await wakeAgent(t, { calls, models: fixture.listing });
    const host = signIns(agent.config.models);
    const gone = client(agent, host);
    const dropped = (await gone.start("chatgpt", "subscription")).login;

    // The client went away; it starts again on the same host.
    const again = client(agent, host);
    const second = (await again.start("chatgpt", "subscription")).login;
    assert.equal(fixture.dropped, 1, "the first sign-in's flow ended");
    assert.deepEqual(await again.reply(dropped, fixture.code), {
        type: "failed",
        login: dropped,
        words: "No sign-in of yours is open.",
    });

    // The host was killed: what it held is gone.
    const restarted = client(agent, signIns(agent.config.models));
    assert.deepEqual(await restarted.reply(second, fixture.code), {
        type: "failed",
        login: second,
        words: "No sign-in of yours is open.",
    });
    const third = (await restarted.start("chatgpt", "subscription")).login;
    for (let i = 0; i < 3; i += 1) await restarted.next(third);
    assert.deepEqual(await restarted.reply(third, fixture.code), {
        type: "done",
        login: third,
    });
    assert.equal(file.get("chatgpt")?.type, "oauth");
    assert.equal(
        sent.filter((call) => call.name === "keep_credential").length,
        1,
    );
    assertNowhere([fixture.code, ...Object.values(fixture.before)], {
        "sign-in answers": [gone.answers, again.answers, restarted.answers],
        log: agent.reported,
        events: agent.events,
        database: await dumped(agent),
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
    await agent.actions.stop(agent.by(from.client), { id: "stop-1" });
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

test("a step waits for its limit, then answers the step it gave", async (t) => {
    const fixture = subscription(t);
    const prompted = Promise.withResolvers<void>();
    fixture.beforePrompt = prompted.promise;
    const { calls } = credentialsFile();
    const agent = await wakeAgent(t, { calls, models: fixture.listing });
    let limit = Promise.withResolvers<void>();
    const hand = client(
        agent,
        signIns(agent.config.models, { waited: () => limit.promise }),
    );
    const login = (await hand.start("chatgpt", "subscription")).login;
    await hand.next(login);
    await hand.next(login);

    // The provider gives nothing more until it asks for the code.
    const waiting = hand.next(login);
    limit.resolve();
    assert.deepEqual(await waiting, {
        type: "step",
        login,
        step: { progress: "Waiting for the offline code" },
    });
    limit = Promise.withResolvers<void>();
    prompted.resolve();
    assert.deepEqual(await hand.next(login), {
        type: "step",
        login,
        step: { prompt: "Paste the code.", secret: true },
    });
    assert.deepEqual(await hand.reply(login, fixture.code), {
        type: "done",
        login,
    });
    await agent.stop();
});

test("a cancel answers a call waiting for its step", async (t) => {
    const fixture = subscription(t);
    fixture.beforePrompt = new Promise(() => {});
    const { file, calls } = credentialsFile();
    const agent = await wakeAgent(t, { calls, models: fixture.listing });
    const hand = client(
        agent,
        // The limit never passes: only the cancel can answer.
        signIns(agent.config.models, { waited: () => new Promise(() => {}) }),
    );
    const login = (await hand.start("chatgpt", "subscription")).login;
    await hand.next(login);
    await hand.next(login);

    const waiting = hand.next(login);
    await hand.cancel(login);

    assert.deepEqual(await waiting, {
        type: "failed",
        login,
        words: "Sign-in cancelled.",
    });
    assert.equal(fixture.dropped, 1);
    assert.equal(file.get("chatgpt"), undefined);
    await agent.stop();
});

test("a typed answer joins the scrub list only when it was secret", async (t) => {
    const key = secret(t, "key");
    const { calls } = credentialsFile();
    const core = createFauxCore({
        provider: "profiled",
        models: [{ id: "large", name: "Large", contextWindow: 64_000 }],
    });
    core.setResponses([
        () => fauxAssistantMessage(`Profile default, key ${key}.`),
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
    assert.deepEqual(await hand.reply(login, "default"), {
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
    assert.equal(answer?.text, "Profile default, key [REDACTED].");
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

test("A15: a check's refusals are today's plain words", async (t) => {
    const { calls } = credentialsFile();
    const answers = ["HTTP 401: no", "HTTP 402: no", "HTTP 403: no"];
    const agent = await wakeAgent(t, {
        calls,
        models: loopback(() =>
            fauxAssistantMessage("", {
                stopReason: "error",
                errorMessage: answers.shift() ?? "",
            }),
        ),
    });
    const keys = keysOf(
        { credential: async () => ({ type: "key", key: secret(t, "key") }) },
        () => {},
    );
    const words = [];
    for (let i = 0; i < 3; i += 1) {
        words.push(
            (await checkModel(agent.config.models, keys, "loopback::large"))
                .words,
        );
    }
    assert.deepEqual(words, [
        "That key was not accepted. Check it was copied whole and is still active.",
        "Your provider account has no credit left. Add some, then try again.",
        "That key was not accepted. Check it was copied whole and is still active.",
    ]);
    await agent.stop();
});
