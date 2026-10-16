import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";
import { connect } from "../src/connection.ts";
import { openHome } from "../src/home.ts";
import { LimitsDoc } from "../src/limits.ts";
import { FAUX_MODEL } from "../src/models.ts";
import type { Item } from "../src/protocol.ts";
import { HOME } from "../src/record.ts";
import { hamlet, inProcess, server } from "./cli-support.ts";
import { closeAfter, context, fauxModels, scratch } from "./support.ts";

/** A connection to the server at `url` of `home`, which has said hello and opened the mayor's chat. */
async function viewer(home: string, url: string) {
    const connection = await connect(url);
    closeAfter(() => connection.close());
    const { token } = (
        JSON.parse(readFileSync(join(home, "credentials.json"), "utf8")) as {
            connection: { token: string };
        }
    ).connection;
    connection.send({ type: "hello", v: 0, token });
    expect(await connection.next()).toMatchObject({ type: "state" });
    connection.send({
        type: "open",
        id: crypto.randomUUID(),
        chat: { with: { hamlet: "home", agent: "mayor" } },
    });
    const items: Item[] = [];
    void (async () => {
        for (;;) {
            const message = await connection.next().catch(() => undefined);
            if (message === undefined) return;
            if (message.type === "transcript") items.push(...message.items);
            if (message.type === "turn") {
                for (const op of message.ops) {
                    if (op.op === "add") items.push(op.item);
                    if (op.op === "close") {
                        const tool = items.find(
                            (item) => item.id === op.item,
                        )?.tool;
                        if (tool) tool.exit = op.exit;
                    }
                }
            }
        }
    })();
    return { connection, items };
}

describe("the hamlet command", () => {
    it("starts the two-line command shim", async () => {
        const run = hamlet(["--help"], scratch());
        expect(await run.ended).toBe(0);
        expect(run.out()).toMatch(/^usage: hamlet/);
        expect(run.err()).toBe("");
    });
    it("reads its flags and names its mistakes (main.rs:189)", async () => {
        const home = scratch();

        const unknown = inProcess(["server", "--hedless"], home);
        expect(await unknown.ended).toBe(1);
        expect(unknown.err()).toContain("unknown argument `--hedless`");
        expect(unknown.err()).toContain("usage: hamlet");

        for (const args of [
            ["server", "--port", "0", "-h"],
            ["--help", "chat"],
            ["chat", "--to", "-h"],
        ]) {
            const help = inProcess(args, home);
            expect(await help.ended).toBe(0);
            expect(help.out()).toMatch(/^usage: hamlet/);
        }

        for (const [args, words] of [
            [["server", "--port"], "--port needs a port number"],
            [["server", "--faux"], "--faux needs the path of a script"],
            [["chat", "--to"], "--to needs an agent's name"],
            [
                ["server", "--port", "high"],
                "--port needs a port number, not `high`",
            ],
            [["serve"], "unknown argument `serve`"],
            [["chat", "--port", "1"], "unknown argument `--port`"],
        ]) {
            const run = inProcess(args as string[], home);
            expect(await run.ended).toBe(1);
            expect(run.err()).toContain(words as string);
        }
    }, 30_000);

    it("refuses a second server on a home that is served (S2)", async () => {
        const home = scratch();
        await server(home, { responses: [] });

        const second = inProcess(["server", "--port", "0"], home);

        expect(await second.ended).toBe(1);
        expect(second.err()).toContain(`A server already runs for ${home}.`);
    });

    it("chats with the mayor from lines on stdin: answers on stdout, the rest on stderr, and the server keeps serving (S13, S1)", async () => {
        const home = scratch();
        const served = await server(home, {
            responses: [
                {
                    calls: [
                        { name: "bash", arguments: { command: "echo hi" } },
                    ],
                },
                "It says hi.",
                "Nothing else.",
            ],
        });

        const chat = inProcess(
            ["chat"],
            home,
            "What does it say?\n\nAnything else?\n",
        );

        expect(await chat.ended).toBe(0);
        expect(chat.out()).toBe("It says hi.\nNothing else.\n");
        expect(chat.err()).toContain('\n  [bash] {"command":"echo hi"}\n');
        expect(served.child.exitCode).toBeNull();
        const found = readFileSync(join(home, "server.json"), "utf8");
        expect(JSON.parse(found)).toEqual({
            url: served.url,
            pid: served.child.pid,
        });
        const { token } = JSON.parse(
            readFileSync(join(home, "credentials.json"), "utf8"),
        ).connection;
        expect(found).not.toContain(token);
        await viewer(home, served.url);
    });

    it.each(["", "Hello.\n"])(
        "exits nonzero when its target does not exist, with stdin %j (S13)",
        async (input) => {
            const home = scratch();
            const served = await server(home, { responses: [] });
            const chat = inProcess(["chat", "--to", "nobody"], home, input);
            let ended: number | string | undefined;
            void chat.ended.then((value) => {
                ended = value;
            });

            await vi.waitFor(() => expect(ended).toBe(1), { timeout: 3000 });
            expect(chat.out()).toBe("");
            expect(chat.err()).toContain("no agent named `nobody`");
            expect(served.child.exitCode).toBeNull();
        },
    );

    it("prints a notice meant for people on stderr", async () => {
        const home = scratch();
        // A budget of nothing stops the turn before it reaches the model.
        const { models } = fauxModels();
        const opened = await openHome(
            home,
            { models, chief: "chief", env: { HAMLET_MODEL: FAUX_MODEL } },
            context,
        );
        await opened.harness.commit(async (tx) => {
            (await tx.doc(LimitsDoc, HOME, HOME)).budget.cost = 0;
        }, context);
        await opened.close(context);
        await server(home, { responses: ["Never sent."] });

        const chat = inProcess(["chat"], home, "Hello.\n");

        expect(await chat.ended).toBe(0);
        expect(chat.out()).toBe("");
        expect(chat.err()).toMatch(/\n {2}\(.+\)\n/);
    });

    it("stops in order at SIGINT, and the next start carries on the turn it cut off (S3)", async () => {
        const home = scratch();
        const first = await server(home, {
            responses: [
                {
                    calls: [
                        { name: "bash", arguments: { command: "sleep 5" } },
                    ],
                },
            ],
        });
        const before = await viewer(home, first.url);
        before.connection.send({
            type: "say",
            id: crypto.randomUUID(),
            to: { hamlet: "home", agent: "mayor" },
            text: "Sleep.",
        });
        await vi.waitFor(() =>
            expect(before.items.map((item) => item.kind)).toContain("tool"),
        );

        first.child.kill("SIGINT");

        expect(await first.ended).toBe(0);
        expect(existsSync(join(home, "server.json"))).toBe(false);
        const again = await server(home, { responses: ["Carried on."] });
        const after = await viewer(home, again.url);
        await vi.waitFor(
            () =>
                expect(after.items.at(-1)).toMatchObject({
                    kind: "assistant",
                    text: "Carried on.",
                }),
            { timeout: 10_000 },
        );
        expect(
            after.items.find((item) => item.kind === "tool")?.tool?.exit,
        ).not.toBe(0);
    });

    it("exits at once at a second SIGINT", async () => {
        const home = scratch();
        const served = await server(home, {
            responses: [{ stallMs: 30_000, text: "Too late." }],
        });
        const { connection, items } = await viewer(home, served.url);
        connection.send({
            type: "say",
            id: crypto.randomUUID(),
            to: { hamlet: "home", agent: "mayor" },
            text: "Take your time.",
        });
        await vi.waitFor(() => expect(items).toHaveLength(1));
        await new Promise((resolve) => setTimeout(resolve, 300));

        served.child.kill("SIGINT");
        await new Promise((resolve) => setTimeout(resolve, 500));
        expect(served.child.exitCode).toBeNull();
        served.child.kill("SIGINT");

        const started = Date.now();
        expect(await served.ended).toBe(130);
        expect(Date.now() - started).toBeLessThan(2000);
    });
});
