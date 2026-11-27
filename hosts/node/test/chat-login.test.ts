import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import { chatActions } from "../src/agent/chat.ts";
import { keysOf } from "../src/agent/credentials.ts";
import { fauxModels } from "../src/agent/faux.ts";
import { installationOf, signIns } from "../src/agent/login.ts";
import type { Login, LoginAnswer } from "../src/agent/messages/agent.ts";
import { checkModel } from "../src/agent/models.ts";
import {
    credentialsFile,
    dumped,
    from,
    named,
    wakeAgent,
} from "./agent-testing.ts";
import {
    assertNowhere,
    client,
    keySignIn,
    loopback,
    secret,
    subscription,
} from "./login-testing.ts";
import { fixture, scratchFolder } from "./testing.ts";

test("a hire signs no one in: sign-in is the Hand's alone, on a host with sign-ins or without, in agent/login.not_the_hand.answer", async (t) => {
    const { calls, sent } = credentialsFile();
    const agent = await wakeAgent(t, {
        calls,
        key: "9",
        input: {
            kind: "hire",
            task: "standing-9",
            place: {
                version: 1,
                lead: 1,
                hires: [],
                roads: [],
                tools_off: [],
                manual: false,
                agrees_roads: false,
            },
            copies: {},
        },
        models: loopback(() => fauxAssistantMessage("Hello.")),
    });
    t.after(() => agent.stop());
    const message: Login = {
        id: "00000000-0000-4000-8000-0000000000dd",
        start: { provider: "loopback", method: "key" },
    };

    for (const config of [
        { ...agent.config, signIns: signIns(agent.config.models) },
        agent.config,
    ]) {
        assert.deepEqual(
            await chatActions(config).login(agent.by(from.client), message),
            fixture("agent/login.not_the_hand.answer"),
        );
    }
    assert.deepEqual(named(sent, "keep_credential"), []);
});

test("A13: a key sign-in's refusals and its repeats are worded", async (t) => {
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

test("a host signs in to a subscription as its installation, the id in its realm folder's credentials.json", async (t) => {
    const home = scratchFolder(t);
    writeFileSync(
        join(home, "credentials.json"),
        JSON.stringify({ installationId: "stable-installation" }),
    );
    const { models } = fauxModels(undefined, true);
    const { file, calls } = credentialsFile();
    const agent = await wakeAgent(t, { calls, models: () => models });
    const hand = client(agent, signIns(models, installationOf(home)));

    let step = await hand.start("chatgpt", "subscription");
    const { login } = step;
    while (step.type === "step" && !("prompt" in step.step))
        step = await hand.next(login);
    assert.deepEqual(await hand.reply(login, "the-code"), {
        type: "done",
        login,
    });
    const stored = file.get("chatgpt");
    assert.equal(stored?.type, "oauth");
    assert.equal(
        JSON.parse(stored.login.extra ?? "{}").clientId,
        "stable-installation",
    );
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

test("A15: a check's refusals are plain words", async (t) => {
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

test("a start repeated after a reply answers as it first did", async (t) => {
    const key = secret(t, "key");
    const { file, calls } = credentialsFile();
    let begun = 0;
    const agent = await wakeAgent(t, {
        calls,
        models: keySignIn("profiled", async (interaction) => {
            begun += 1;
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
        }),
    });
    const given = signIns(agent.config.models);
    const hand = client(agent, given);
    const first = await hand.start("profiled", "key");
    await hand.reply(first.login, "default");
    const repeated = await chatActions({
        ...agent.config,
        signIns: given,
    }).login(agent.by(from.client), {
        id: first.login,
        start: { provider: "profiled", method: "key" },
    });

    assert.deepEqual(repeated, first);
    assert.equal(begun, 1, "the repeat began no flow");
    assert.deepEqual(await hand.reply(first.login, key), {
        type: "done",
        login: first.login,
    });
    assert.deepEqual(file.get("profiled"), { type: "key", key });
    await agent.stop();
});

test("a sign-in with no first step in time fails with its words", async (t) => {
    const { calls } = credentialsFile();
    const agent = await wakeAgent(t, {
        calls,
        models: keySignIn("silent", () => new Promise(() => {})),
    });
    const hand = client(
        agent,
        signIns(agent.config.models, { waited: async () => {} }),
    );
    const answer = await hand.start("silent", "key");

    assert.deepEqual(answer, {
        type: "failed",
        login: answer.login,
        words: "The provider did not answer in time.",
    });
    assert.deepEqual(await hand.next(answer.login), {
        type: "failed",
        login: answer.login,
        words: "No sign-in of yours is open.",
    });
    await agent.stop();
});
