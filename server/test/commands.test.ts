import { randomUUID } from "node:crypto";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import { createModels, createProvider } from "@earendil-works/pi-ai/models";
import {
    createFauxCore,
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { ChatDoc, ChiefEntry } from "../src/chats.ts";
import { COMMANDS, readCommand } from "../src/commands.ts";
import type { Home } from "../src/home.ts";
import { LimitsDoc } from "../src/limits.ts";
import { say } from "../src/say.ts";
import { replyFor } from "../src/server.ts";
import {
    say as chiefSay,
    context,
    fauxModels,
    mayorChat,
    openWaits,
    scratch,
    setFolder,
    testHome,
    transcript,
} from "./support.ts";

it("names each typed command once and reads its case-sensitive first word", () => {
    const names = COMMANDS.map((command) => command.name);
    expect(new Set(names).size).toBe(names.length);
    for (const command of COMMANDS) {
        expect(command.name).toMatch(/^\//);
        expect(command.description.length).toBeGreaterThan(0);
    }
    expect(readCommand("  /deny   not on this machine  ")).toEqual({
        command: "answer",
        choice: "deny",
        text: "not on this machine",
    });
    expect(readCommand("/queue")).toEqual({ command: "queue" });
    expect(readCommand("/undo")).toEqual({
        command: "deed",
        deed: "undo",
        rest: "",
    });
    expect(readCommand("/stop")).toEqual({ command: "stop", agent: null });
    expect(readCommand("/stop all")).toEqual({ command: "stop", agent: null });
    expect(readCommand("/stop scout")).toEqual({
        command: "stop",
        agent: "scout",
    });
    expect(readCommand("/revoke a network")).toEqual({
        command: "revoke",
        agent: "a",
        permission: "network",
    });
    for (const words of [
        "hello /new",
        "/nothing",
        "/NEW",
        "/new words",
        "/reflect words",
        "/retry words",
        "/budget words",
    ])
        expect(readCommand(words)).toBeUndefined();
});

it("answers a decree without a model call or a first conversation", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    expect(await typed(home, "/undo")).toMatchObject({
        type: "reply",
        kind: "notice",
        text: "Nothing to undo. Only a removal or a merge can be undone (the archive keeps the house); a permission is taken back with /revoke. A command that ran, a file that was changed and what was spent cannot be undone.",
    });
    expect(
        (await home.harness.snapshot(ChatDoc, "home:0", context))?.chats,
    ).toEqual({});
    expect(faux.state.callCount).toBe(0);
});

it("sends unknown slash lines and session commands followed by words as scrubbed chief words", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    await home.credentials.modify("test", async () => ({
        type: "api_key",
        key: "secret-typed-key",
    }));
    const words = [
        "/nothing",
        "/new words",
        "/reflect words",
        "/retry words",
        "/budget words",
        "My key is secret-typed-key.",
    ];
    faux.setResponses(words.map(() => fauxAssistantMessage("Answer.")));
    for (const text of words) {
        expect(await typed(home, text)).toBeUndefined();
        await (await mayorChat(home)).waitForIdle(context);
    }
    expect(
        (await transcript(await mayorChat(home)))
            .filter(ChiefEntry.is)
            .map((entry) => entry.data.text),
    ).toEqual([
        "/nothing",
        "/new words",
        "/reflect words",
        "/retry words",
        "/budget words",
        "My key is [REDACTED].",
    ]);
    const messages = (await (await mayorChat(home)).context(context)).messages;
    expect(
        messages.filter((message) => message.role === "user").at(-1)?.content,
    ).toBe("My key is [REDACTED].");
    expect(faux.state.callCount).toBe(6);
});

async function typed(home: Home, text: string, agent = "mayor") {
    const message = {
        type: "say",
        id: randomUUID(),
        to: { hamlet: "home", agent },
        text,
    } as const;
    const outcome = await say(home, message, context);
    return outcome === undefined
        ? undefined
        : replyFor(message.id, outcome, home.secrets);
}

it("holds plain words at an approval and answers it from any chat without starting that chat", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    await setFolder(home, scratch());
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("bash", { command: "echo allowed" }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("Done."),
    ]);
    const run = await chiefSay(home, "Run.");
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(1));
    expect(await typed(home, "More words.")).toMatchObject({
        kind: "notice",
        text: "Not sent. mayor is waiting for your answer:\n[approval] mayor wants to run `echo allowed`\n    echo allowed\n    answer with: /allow /always /deny",
    });
    await expect(typed(home, "/go")).rejects.toThrow(
        "That cannot be answered that way:\n[approval] mayor wants to run `echo allowed`\n    echo allowed\n    answer with: /allow /always /deny",
    );
    expect(await typed(home, "/queue", "nowhere")).toMatchObject({
        text: expect.stringContaining(
            "[approval] mayor wants to run `echo allowed`",
        ),
    });
    expect(faux.state.callCount).toBe(1);
    expect(await typed(home, "/allow", "nowhere")).toMatchObject({
        text: "Answered /allow: mayor wants to run `echo allowed`",
    });
    expect(await run.wait(context)).toMatchObject({ status: "done" });
    expect(await typed(home, "/queue", "nowhere")).toMatchObject({
        text: "Nothing waits for you.",
    });
    expect(
        (await transcript(await mayorChat(home))).filter(ChiefEntry.is),
    ).toHaveLength(1);
    expect(faux.state.callCount).toBe(2);
    expect(
        Object.keys(
            (await home.harness.snapshot(ChatDoc, "home:0", context))?.chats ??
                {},
        ),
    ).toEqual([home.chief]);
});

it("dismisses a fault through the same answer deed and leaves usage errors out of the chat", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    faux.setResponses([
        fauxAssistantMessage("", {
            stopReason: "error",
            errorMessage: "Bad request",
        }),
    ]);
    await (await chiefSay(home, "Fail.")).wait(context);
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(1));
    expect(await typed(home, "/dismiss")).toMatchObject({
        text: "Answered /dismiss: mayor failed: Bad request",
    });
    expect(await openWaits(home)).toEqual([]);
    expect(await typed(home, "/reply")).toMatchObject({
        kind: "notice",
        text: "say what to reply: /reply <text>",
    });
    expect(faux.state.callCount).toBe(1);
});

it.each(["founder", "mayor", "all"])(
    "stops the mayor through /stop %s and does not stop it for an unknown name",
    async (target) => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        await setFolder(home, scratch());
        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("bash", { command: "echo never" }),
                { stopReason: "toolUse" },
            ),
        ]);
        const run = await chiefSay(home, "Run.");
        await vi.waitFor(async () =>
            expect(await openWaits(home)).toHaveLength(1),
        );
        await expect(typed(home, "/stop nobody")).rejects.toThrow(
            "no agent named `nobody`",
        );
        expect(await openWaits(home)).toHaveLength(1);
        expect(await typed(home, `/stop ${target}`)).toMatchObject({
            text:
                target === "all"
                    ? "Stopping everything."
                    : `Stopping ${target}.`,
        });
        expect(await run.wait(context)).toMatchObject({ status: "unanswered" });
        expect(await openWaits(home)).toEqual([]);
        expect(faux.state.callCount).toBe(1);
    },
);

it("routes new, resume, reflect and retry through the existing conversation deeds", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    faux.setResponses([
        fauxAssistantMessage("Old answer."),
        fauxAssistantMessage("Nothing to save."),
        fauxAssistantMessage("", {
            stopReason: "error",
            errorMessage: "Bad request",
        }),
        fauxAssistantMessage("Carried on."),
    ]);
    await (await chiefSay(home, "A past conversation.")).wait(context);
    const old = await mayorChat(home);
    expect(await typed(home, "/new")).toMatchObject({
        text: "Started a new session.",
    });
    expect((await mayorChat(home)).id).not.toBe(old.id);
    expect(await typed(home, "/resume")).toMatchObject({
        text: expect.stringContaining("1: A past conversation."),
    });
    expect(await typed(home, "/resume wrong")).toMatchObject({
        text: "Say which: `/resume <number>`, with the number History shows for a past session.",
    });
    expect(await typed(home, "/resume 99")).toMatchObject({
        kind: "notice",
        text: "There is no past session `99` here.",
    });
    expect(await typed(home, "/resume 1")).toMatchObject({
        text: "Went back to an earlier session.",
    });
    expect((await mayorChat(home)).id).toBe(old.id);
    expect(await typed(home, "/reflect")).toBeUndefined();
    await old.waitForIdle(context);
    await (await chiefSay(home, "Fail.")).wait(context);
    expect(await typed(home, "/retry")).toBeUndefined();
    await old.waitForIdle(context);
    expect(
        (await transcript(old)).filter(
            (entry) => entry.kind === "hamlet.retry",
        ),
    ).toHaveLength(1);
    await expect(typed(home, "/retry")).rejects.toThrow(
        "the last message was answered, so there is nothing to carry on",
    );
    expect(faux.state.callCount).toBe(4);
});

it("reports spend at once and resets the period without asking the model", async () => {
    const { models, faux } = meteredModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    expect(await typed(home, "/budget")).toMatchObject({
        text: "Hamlet today: $0.00 of $5.00\n- mayor: faux::faux-1, $0.00",
    });
    expect(
        (await home.harness.snapshot(ChatDoc, "home:0", context))?.chats,
    ).toEqual({});
    faux.setResponses([fauxAssistantMessage("Done.")]);
    await (await chiefSay(home, "Spend.")).wait(context);
    expect(await typed(home, "/budget")).toMatchObject({
        text: "Hamlet today: $0.20 of $5.00\n- mayor: faux::faux-1, $0.20",
    });
    expect(await typed(home, "/reset-budget")).toMatchObject({
        text: "The budget starts again from here. What was spent before is kept in the ledger.",
    });
    expect(await typed(home, "/budget")).toMatchObject({
        text: "Hamlet today: $0.00 of $5.00\n- mayor: faux::faux-1, $0.00",
    });
    expect(faux.state.callCount).toBe(1);
});

it("lists permissions from both limits and grants and revokes each form honestly", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    expect(await typed(home, "/grants")).toMatchObject({
        text: "No agent holds a permission.",
    });
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("bash", { command: "echo net", network: true }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("Done."),
    ]);
    const run = await chiefSay(home, "Ask for the network.");
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(1));
    expect(await typed(home, "/always")).toMatchObject({
        text: "Answered /always: mayor wants the network for `echo net`",
    });
    await run.wait(context);
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).permissions.unconfined = [
            "mayor",
        ];
    }, context);
    expect(await typed(home, "/grants")).toMatchObject({
        text: "mayor may use: unconfined (limits.toml)\nmayor may use: network (given by chief; /revoke mayor network)",
    });
    expect(await typed(home, "/revoke founder network")).toMatchObject({
        text: "`founder` no longer has network.",
    });
    expect(await typed(home, "/revoke mayor unconfined")).toMatchObject({
        text: "Revoked what was granted. `mayor` still has unconfined under [permissions] in limits.toml.",
    });
    expect(await typed(home, "/grants")).toMatchObject({
        text: "mayor may use: unconfined (limits.toml)",
    });
    expect(await typed(home, "/revoke mayor network")).toMatchObject({
        text: "`mayor` no longer has network.",
    });
    expect(await typed(home, "/revoke mayor")).toMatchObject({
        text: "usage: /revoke <agent> <network|unconfined>",
    });
    expect(faux.state.callCount).toBe(2);
});

function meteredModels() {
    const faux = createFauxCore({ models: [{ id: "faux-1" }] });
    const models = createModels();
    const stream: typeof faux.stream = (model, input, options) => {
        const source = faux.stream(model, input, options);
        const out = createAssistantMessageEventStream();
        void (async () => {
            for await (const event of source) {
                if (event.type === "done") {
                    event.message.usage = {
                        input: 1000,
                        output: 1000,
                        cacheRead: 0,
                        cacheWrite: 0,
                        totalTokens: 2000,
                        cost: {
                            input: 0.1,
                            output: 0.1,
                            cacheRead: 0,
                            cacheWrite: 0,
                            total: 0.2,
                        },
                    };
                }
                out.push(event);
            }
            out.end(await source.result());
        })();
        return out;
    };
    models.setProvider(
        createProvider({
            id: "faux",
            models: faux.models,
            auth: {
                apiKey: { name: "Faux", resolve: async () => ({ auth: {} }) },
            },
            api: { stream, streamSimple: stream },
        }),
    );
    return { models, faux };
}

it("shows the oldest waits with elapsed seconds and scrubs a typed refusal before it is recorded", async () => {
    const { models, faux } = fauxModels();
    let at = 1000;
    const home = await testHome(scratch(), {
        models,
        now: () => at,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    await home.credentials.modify("test", async () => ({
        type: "api_key",
        key: "secret-in-reason",
    }));
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("bash", { command: "echo first", network: true }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage(
            fauxToolCall("bash", { command: "echo second", network: true }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("First done."),
        (request) => {
            expect(JSON.stringify(request.messages)).toContain(
                "the chief refused it: [REDACTED]",
            );
            expect(JSON.stringify(request.messages)).not.toContain(
                "secret-in-reason",
            );
            return fauxAssistantMessage("Second done.");
        },
    ]);
    const first = await chiefSay(home, "First.");
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(1));
    at = 2000;
    const second = await chiefSay(home, "Second.", "another-chief");
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(2));
    at = 11000;
    expect(await typed(home, "/queue", "nowhere")).toMatchObject({
        text: "[approval] mayor wants the network for `echo first`\n    echo first\n    answer with: /allow /always /deny (waited 10s, asked 1)\n[approval] mayor wants the network for `echo second`\n    echo second\n    answer with: /allow /always /deny (waited 9s, asked 1)",
    });
    expect(await typed(home, "/allow", "nowhere")).toMatchObject({
        text: "Answered /allow: mayor wants the network for `echo first`",
    });
    await first.wait(context);
    expect(
        await typed(home, "/deny secret-in-reason", "nowhere"),
    ).toMatchObject({
        text: "Answered /deny: mayor wants the network for `echo second`",
    });
    await second.wait(context);
    expect(await typed(home, "/queue")).toMatchObject({
        text: "Nothing waits for you.",
    });
});

it("holds words to founder at its mayor's wait and answers that conversation before the oldest wait", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    await setFolder(home, scratch());
    faux.setResponses([
        fauxAssistantMessage(fauxToolCall("bash", { command: "echo older" }), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage(fauxToolCall("bash", { command: "echo own" }), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("Own done."),
        fauxAssistantMessage("Older done."),
    ]);
    const older = await chiefSay(home, "Older.", "another-chief");
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(1));
    const own = await chiefSay(home, "Own.");
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(2));
    expect(await typed(home, "More words.", "founder")).toMatchObject({
        text: "Not sent. mayor is waiting for your answer:\n[approval] mayor wants to run `echo own`\n    echo own\n    answer with: /allow /always /deny",
    });
    expect(await typed(home, "/allow", "founder")).toMatchObject({
        text: "Answered /allow: mayor wants to run `echo own`",
    });
    await own.wait(context);
    expect((await openWaits(home)).map((wait) => wait.about)).toEqual([
        "echo older",
    ]);
    await typed(home, "/allow");
    await older.wait(context);
    expect(
        (await transcript(await mayorChat(home))).filter(ChiefEntry.is),
    ).toHaveLength(1);
});
