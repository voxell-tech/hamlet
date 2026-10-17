import { randomUUID } from "node:crypto";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import { createProvider } from "@earendil-works/pi-ai/models";
import {
    createFauxCore,
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { COMMANDS } from "../src/commands.ts";
import { hamlet, server, tty } from "./cli-support.ts";
import { changes, mayor, request, say, served } from "./connection-support.ts";
import { fauxModels, scratch, setFolder } from "./support.ts";

it("prints standing waits once at connect and new waits with their answer strips", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    await setFolder(run.home, scratch());
    faux.setResponses([
        fauxAssistantMessage(fauxToolCall("bash", { command: "echo first" }), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage(fauxToolCall("bash", { command: "echo second" }), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("Done."),
    ]);
    say(run.connection, "Run both.");
    await vi.waitFor(() =>
        expect(changes(run.messages).some((item) => item.type === "wait")).toBe(
            true,
        ),
    );
    const chat = hamlet(["chat"], run.home.dir, undefined, {
        HAMLET_SERVER: run.server.url,
        HAMLET_TOKEN: run.home.credentials.token,
    });
    await vi.waitFor(
        () =>
            expect(chat.err()).toContain(
                "NEEDS YOU: [approval] mayor wants to run `echo first`",
            ),
        { timeout: 5000 },
    );
    expect(chat.err()).toContain(
        "    echo first\n    answer with: /allow /always /deny",
    );
    chat.child.stdin.write("/allow\n");
    await vi.waitFor(() =>
        expect(chat.err()).toContain(
            "NEEDS YOU: [approval] mayor wants to run `echo second`",
        ),
    );
    chat.child.stdin.write("/allow\n");
    await vi.waitFor(() => expect(chat.out()).toBe("Done.\n"));
    chat.child.stdin.end();
    expect(await chat.ended).toBe(0);
    expect(chat.err().match(/NEEDS YOU:/g)).toHaveLength(2);
    expect(chat.out()).toBe("Done.\n");
});

it("prints answers alone on stdout and dims reasoning, notices, tool exits and errors on stderr", async () => {
    const home = scratch();
    await server(home, {
        responses: [
            {
                thinking: "Let me see.",
                calls: [
                    {
                        name: "bash",
                        arguments: { command: "echo broken; exit 7" },
                    },
                ],
            },
            "Could not do it.",
            "",
        ],
    });
    const chat = hamlet(["chat"], home, "Try it.\n/mode fast\n/undo\nFail.\n");
    expect(await chat.ended).toBe(0);
    expect(chat.out()).toBe("Could not do it.\n");
    expect(chat.err()).toContain("\x1b[2mLet me see.\x1b[0m");
    expect(chat.err()).toContain('  [bash] {"command":"echo broken; exit 7"}');
    expect(chat.err()).toContain("  [exit 7]\n");
    expect(chat.err()).toContain("error: usage: /mode <manual|auto>");
    expect(chat.err()).toContain("  (Nothing to undo.");
    expect(chat.err()).toContain(
        "error: the model answered with nothing. Send the message again",
    );
});

it("switches against the realm's state and answers unknown names and missing names without sending them", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    faux.setResponses([
        fauxAssistantMessage("First."),
        fauxAssistantMessage("Second."),
    ]);
    const chat = hamlet(
        ["chat"],
        run.home.dir,
        "/agent founder\nOne.\n/hamlet home\nTwo.\n/agent nobody\n/hamlet nowhere\n/agent\n/hamlet\n",
        {
            HAMLET_SERVER: run.server.url,
            HAMLET_TOKEN: run.home.credentials.token,
        },
    );
    expect(await chat.ended).toBe(0);
    expect(chat.out()).toBe("First.\nSecond.\n");
    expect(chat.err()).toContain("  (talking to the agent founder)");
    expect(chat.err()).toContain("  (talking to the mayor of home)");
    expect(chat.err()).toContain("no agent named `nobody`");
    expect(chat.err()).toContain("no hamlet named `nowhere`");
    expect(chat.err()).toContain("usage: /agent <name>");
    expect(chat.err()).toContain("usage: /hamlet <name>");
    expect(faux.state.callCount).toBe(2);
});

it("explains keys and a hello on a home that has no saved settings or model override", async () => {
    const { models } = fauxModels();
    const run = await served({ models, env: { HAMLET_MODEL: "" } });
    const chat = hamlet(["chat"], run.home.dir, "", {
        HAMLET_SERVER: run.server.url,
        HAMLET_TOKEN: run.home.credentials.token,
    });
    expect(await chat.ended).toBe(0);
    expect(chat.err()).toContain("hamlet login");
    expect(chat.err()).toContain("hamlet models try");
    expect(chat.out()).toBe("");
});

it("waits at EOF for its own lines, not a later line from another client, and leaves that line running (S1)", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    let first: (() => void) | undefined;
    let second: (() => void) | undefined;
    faux.setResponses([
        async () => {
            await new Promise<void>((resolve) => {
                first = resolve;
            });
            return fauxAssistantMessage("Mine.");
        },
        async () => {
            await new Promise<void>((resolve) => {
                second = resolve;
            });
            return fauxAssistantMessage("Theirs.");
        },
    ]);
    const chat = hamlet(["chat"], run.home.dir, "My line.\n", {
        HAMLET_SERVER: run.server.url,
        HAMLET_TOKEN: run.home.credentials.token,
    });
    await vi.waitFor(() => expect(first).toBeDefined(), { timeout: 5000 });
    let ended: number | string | undefined;
    void chat.ended.then((value) => {
        ended = value;
    });
    expect(ended).toBeUndefined();
    say(run.connection, "Their line.");
    await request(run.connection, run.messages, {
        type: "read",
        id: randomUUID(),
        what: "history",
        agent: mayor,
    });
    first?.();
    await vi.waitFor(() => expect(second).toBeDefined());
    try {
        await vi.waitFor(() => expect(ended).toBe(0));
        expect(chat.out()).toBe("Mine.\n");
    } finally {
        second?.();
    }
});

it("exits at once on Ctrl-C without stopping the mayor, and another chat sees the same conversation (S1)", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    faux.setResponses([
        fauxAssistantMessage(fauxToolCall("bash", { command: "sleep 30" }), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("Stopped."),
    ]);
    const env = {
        HAMLET_SERVER: run.server.url,
        HAMLET_TOKEN: run.home.credentials.token,
    };
    const first = hamlet(["chat"], run.home.dir, undefined, env);
    first.child.stdin.write("Sleep.\n");
    await vi.waitFor(() => expect(first.err()).toContain("[bash]"), {
        timeout: 5000,
    });
    first.child.kill("SIGINT");
    let ended: number | string | undefined;
    void first.ended.then((value) => {
        ended = value;
    });
    await vi.waitFor(() => expect(ended).toBe("SIGINT"));
    const after = await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat: { with: mayor },
    });
    expect(after).toMatchObject({
        type: "transcript",
        working: true,
        items: expect.arrayContaining([
            expect.objectContaining({ kind: "user", text: "Sleep." }),
        ]),
    });
    const second = hamlet(["chat"], run.home.dir, "", env);
    expect(await second.ended).toBe(0);
    expect(second.err()).toContain('[bash] {"command":"sleep 30"}');
    await request(run.connection, run.messages, {
        type: "stop",
        id: randomUUID(),
    });
});

it("shows at most the last answer when another chat connects, without replaying older output", async () => {
    const home = scratch();
    await server(home, { responses: ["First.", "Second."] });
    const first = hamlet(["chat"], home, "One.\nTwo.\n");
    expect(await first.ended).toBe(0);
    expect(first.out()).toBe("First.\nSecond.\n");
    const again = hamlet(["chat"], home, "");
    expect(await again.ended).toBe(0);
    expect(again.out()).toBe("Second.\n");
});

it("shows the chief's available commands on stderr without putting help in the answers", async () => {
    const home = scratch();
    await server(home, { responses: ["Hello."] });
    const chat = hamlet(["chat"], home, "Hello.\n");
    expect(await chat.ended).toBe(0);
    expect(chat.out()).toBe("Hello.\n");
    expect(chat.err()).toContain("Ctrl-D quits");
    expect(
        chat
            .err()
            .split("\n")
            .filter((line) => line.startsWith("  /")),
    ).toEqual(
        COMMANDS.filter(
            ({ deed }) => !["go", "withdraw", "reply"].includes(deed),
        ).map(
            ({ name, args, description }) =>
                `  ${`${name} ${args}`.trimEnd()}: ${description}`,
        ),
    );
});

it("prompts at a real terminal and quits after answering on Ctrl-D", async () => {
    const home = scratch();
    await server(home, { responses: ["Hello."] });
    const result = await tty(["chat"], home, [
        { after: "\n> ", write: "Hello.\n" },
        { after: "Hello.\r\n\r\n> ", write: "\x04" },
    ]);
    expect(result.output).toContain("\n> ");
    expect(result.output.match(/\n> /g)).toHaveLength(2);
    expect(result.code).toBe(0);
}, 15_000);

it("prints the answer to its own line even when /new puts that conversation away before EOF", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    let release: (() => void) | undefined;
    faux.setResponses([
        async () => {
            await new Promise<void>((resolve) => {
                release = resolve;
            });
            return fauxAssistantMessage("From the old conversation.");
        },
        fauxAssistantMessage("From the new conversation."),
    ]);
    const chat = hamlet(["chat"], run.home.dir, undefined, {
        HAMLET_SERVER: run.server.url,
        HAMLET_TOKEN: run.home.credentials.token,
    });
    chat.child.stdin.write("One.\n");
    await vi.waitFor(() => expect(release).toBeDefined(), { timeout: 5000 });
    try {
        chat.child.stdin.write("/new\nTwo.\n");
        await vi.waitFor(() =>
            expect(chat.err()).toContain("Started a new session."),
        );
        chat.child.stdin.end();
    } finally {
        release?.();
    }
    expect(await chat.ended).toBe(0);
    expect(chat.out(), chat.err()).toContain("From the old conversation.\n");
    expect(chat.out().match(/From the new conversation\./g)).toHaveLength(1);
});

it("keeps chatting when a pinned past view missed live items while that session was current", async () => {
    const { models } = fauxModels();
    const faux = createFauxCore({
        models: [{ id: "faux-1" }],
        tokenSize: { min: 1, max: 1 },
        tokensPerSecond: 100,
    });
    const begin = Promise.withResolvers<void>();
    const finish = Promise.withResolvers<void>();
    const stream: typeof faux.stream = (model, input, options) => {
        const out = createAssistantMessageEventStream();
        void (async () => {
            await begin.promise;
            const source = faux.stream(model, input, options);
            let deltas = 0;
            for await (const event of source) {
                // The faux provider shares partial content across events; snapshot before pausing it.
                const snapshot = structuredClone(event);
                if (event.type === "text_delta" && deltas++ > 0)
                    await finish.promise;
                out.push(snapshot);
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
    faux.setResponses([fauxAssistantMessage("From the first session.")]);
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    const chat = hamlet(["chat"], run.home.dir, undefined, {
        HAMLET_SERVER: run.server.url,
        HAMLET_TOKEN: run.home.credentials.token,
    });
    try {
        chat.child.stdin.write("Work.\n/new\n");
        await vi.waitFor(
            () =>
                expect(
                    chat.err().match(/\(Started a new session\.\)/g),
                ).toHaveLength(2),
            { timeout: 5000 },
        );
        chat.child.stdin.write("/resume 1\n");
        await vi.waitFor(() =>
            expect(
                chat.err().match(/\(Went back to an earlier session\.\)/g),
            ).toHaveLength(2),
        );
        begin.resolve();
        await vi.waitFor(() => expect(chat.out()).toContain("From"));
        chat.child.stdin.write("/new\n");
        await vi.waitFor(() =>
            expect(
                chat.err().match(/\(Started a new session\.\)/g),
            ).toHaveLength(4),
        );
        finish.resolve();
        chat.child.stdin.end();
        expect(await chat.ended, chat.err()).toBe(0);
        expect(chat.err()).not.toContain("No item");
        expect(chat.out().replaceAll("\n", "")).toBe("From the first session.");
    } finally {
        begin.resolve();
        finish.resolve();
    }
});
