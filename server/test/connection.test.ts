import { randomUUID } from "node:crypto";
import { once } from "node:events";
import { setTimeout as sleep } from "node:timers/promises";
import { describe, expect, it, onTestFinished, vi } from "vitest";
import { WebSocket } from "ws";
import {
    type Connection,
    connect,
    type Handlers,
    type Server,
    serve,
} from "../src/connection.ts";
import type {
    Changes,
    ClientMessage,
    Hello,
    Reply,
    Say,
    State,
} from "../src/protocol.ts";

const TOKEN = "home-token";
const hello: Hello = { type: "hello", v: 0, token: TOKEN };
const mayor = { hamlet: "home", agent: "mayor" };

/** A state with nothing in it, at `revision`. */
function state(revision = 0): State {
    return {
        type: "state",
        epoch: 1,
        revision,
        chief: "chief",
        sandbox: "Seatbelt",
        hamlets: [],
        agents: [],
        waits: [],
        grants: [],
        settings: {
            model: "local::qwen3.6:35b-a3b",
            reflectEvery: 10,
            allowNet: false,
            endpoints: [],
            unset: true,
            link: { state: "untested", models: null, words: null },
        },
        providers: [],
    };
}

function say(text: string): Say {
    return { type: "say", id: randomUUID(), to: mayor, text };
}

function heard(message: Say): Reply {
    return { type: "reply", to: message.id, kind: "notice", text: "Heard." };
}

/** A server with `handlers`, answering hello with an empty state by default. */
async function start(
    handlers: Partial<Handlers> = {},
    options: { helloMs?: number } = {},
): Promise<Server> {
    const server = await serve({
        token: TOKEN,
        port: 0,
        handlers: { hello: () => state(), ...handlers },
        ...options,
    });
    onTestFinished(() => server.close());
    return server;
}

/** A connection to `server` that has not said hello. */
async function open(server: Server): Promise<Connection> {
    const connection = await connect(server.url);
    onTestFinished(() => connection.close());
    return connection;
}

/** A connection to `server` that has said hello and taken its state. */
async function join(server: Server): Promise<Connection> {
    const connection = await open(server);
    connection.send(hello);
    expect(await connection.next()).toMatchObject({ type: "state" });
    return connection;
}

describe("the server's end of a connection", () => {
    it("closes a connection whose hello has the wrong token, with a fault", async () => {
        const hellos: Hello[] = [];
        const server = await start({
            hello: (message) => {
                hellos.push(message);
                return state();
            },
        });
        const connection = await open(server);

        connection.send({ ...hello, token: "guess" });

        expect(await connection.next()).toEqual({
            type: "fault",
            text: "That is not this server's token.",
        });
        expect(await connection.closed).toBe(1008);
        expect(hellos).toEqual([]);
    });

    it("closes a connection that sends anything before hello, with a fault", async () => {
        const server = await start();
        const early = await open(server);
        const wrong = await open(server);

        early.send(say("Hello?"));
        wrong.send({ ...hello, v: 1 } as unknown as ClientMessage);

        expect(await early.next()).toEqual({
            type: "fault",
            text: "A connection begins with hello and the home's token.",
        });
        expect(await early.closed).toBe(1008);
        expect(await wrong.next()).toEqual({
            type: "fault",
            text: "This `hello` message does not fit protocol v0: `v` must be `0`.",
        });
        expect(await wrong.closed).toBe(1008);
    });

    it("handles nothing a connection sends after the message it was refused for", async () => {
        const calls: string[] = [];
        const server = await start({
            hello: () => {
                calls.push("hello");
                return state();
            },
            say: (message) => {
                calls.push(message.text);
                return heard(message);
            },
        });
        const connection = await open(server);

        connection.send({ ...hello, token: "guess" });
        connection.send(hello);
        connection.send(say("Let me in."));

        expect(await connection.next()).toEqual({
            type: "fault",
            text: "That is not this server's token.",
        });
        expect(await connection.closed).toBe(1008);
        await expect(connection.next()).rejects.toThrow(
            "The connection to the server closed (1008).",
        );
        expect(calls).toEqual([]);
    });

    it("closes a connection that does not say hello in time, with a fault", async () => {
        const server = await start({}, { helloMs: 50 });
        const connection = await open(server);

        expect(await connection.next()).toEqual({
            type: "fault",
            text: "A connection begins with hello and the home's token.",
        });
        expect(await connection.closed).toBe(1008);
    });

    it("closes a connection that sends a message far larger than a chief writes", async () => {
        const hellos: Hello[] = [];
        const server = await start({
            hello: (message) => {
                hellos.push(message);
                return state();
            },
        });
        const connection = await open(server);

        connection.send({ ...hello, token: "x".repeat(1024 * 1024) });

        // 1009 is RFC 6455's close for a message too big to take.
        expect(await connection.closed).toBe(1009);
        expect(hellos).toEqual([]);
    });

    it("hands a valid hello to its handler and sends back the state it returns", async () => {
        const hellos: Hello[] = [];
        const server = await start({
            hello: (message) => {
                hellos.push(message);
                return state(7);
            },
        });
        const connection = await open(server);
        const again = { ...hello, seen: { epoch: 1, revision: 3 } };

        connection.send(again);

        expect(await connection.next()).toEqual(state(7));
        expect(hellos).toEqual([again]);
    });

    it("sends no state when the hello handler returns none", async () => {
        const server = await start({ hello: () => undefined, say: heard });
        const connection = await open(server);
        const said = say("Still there?");

        connection.send(hello);
        connection.send(said);

        expect(await connection.next()).toEqual(heard(said));
    });

    it("gives a malformed message a fault naming it, and keeps the connection open", async () => {
        const server = await start({ say: heard });
        const connection = await join(server);
        const said = say("Now?");

        connection.send({ ...said, id: "1" });
        connection.send({
            ...said,
            text: undefined,
        } as unknown as ClientMessage);
        connection.send(said);

        expect(await connection.next()).toEqual({
            type: "fault",
            text: "This `say` message does not fit protocol v0: `id` must be a UUID.",
        });
        expect(await connection.next()).toEqual({
            type: "fault",
            text: "This `say` message does not fit protocol v0: it lacks `text`.",
            to: said.id,
        });
        expect(await connection.next()).toEqual(heard(said));
    });

    it("sends a message to one connection only, and a broadcast to every open one", async () => {
        const server = await start({ say: heard });
        const one = await join(server);
        const two = await join(server);
        const gone = await join(server);
        await gone.close();
        const said = say("Just me.");
        const changes: Changes = {
            type: "change",
            epoch: 1,
            revision: 1,
            changes: [],
        };

        one.send(said);
        expect(await one.next()).toEqual(heard(said));
        server.broadcast(changes);

        expect(await one.next()).toEqual(changes);
        expect(await two.next()).toEqual(changes);
    });

    it("sends only broadcasts newer than the hello state after it", async () => {
        const changes: Changes = {
            type: "change",
            epoch: 1,
            revision: 8,
            changes: [],
        };
        const server: Server = await start({
            hello: () => {
                server.broadcast(state(6));
                server.broadcast({ ...changes, revision: 7 });
                server.broadcast(changes);
                return state(7);
            },
        });
        const connection = await open(server);

        connection.send(hello);

        expect(await connection.next()).toEqual(state(7));
        expect(await connection.next()).toEqual(changes);
    });

    it("handles a client's messages one at a time, in the order they came", async () => {
        const steps: string[] = [];
        const holding = (message: Say): Reply => ({
            ...heard(message),
            text: "Holding.",
        });
        const server = await start({
            say: async (message, client) => {
                steps.push(`began ${message.text}`);
                if (message.text === "first") {
                    client.send(holding(message));
                    await sleep(50);
                }
                steps.push(`ended ${message.text}`);
                return heard(message);
            },
        });
        const connection = await join(server);
        const first = say("first");
        const second = say("second");

        connection.send(first);
        connection.send(second);

        expect(await connection.next()).toEqual(holding(first));
        expect(await connection.next()).toEqual(heard(first));
        expect(await connection.next()).toEqual(heard(second));
        expect(steps).toEqual([
            "began first",
            "ended first",
            "began second",
            "ended second",
        ]);
    });

    it("answers a request with a fault in its handler's words when the handler fails", async () => {
        const server = await start({
            say: (message) => {
                if (message.text === "/to scout hi") {
                    throw new Error("There is no agent `scout` here.");
                }
                return heard(message);
            },
        });
        const connection = await join(server);
        const failing = say("/to scout hi");
        const said = say("Fine.");

        connection.send(failing);
        connection.send(said);

        expect(await connection.next()).toEqual({
            type: "fault",
            text: "There is no agent `scout` here.",
            to: failing.id,
        });
        expect(await connection.next()).toEqual(heard(said));
    });

    it("answers a message no handler takes with a fault naming it", async () => {
        const server = await start();
        const connection = await join(server);
        const id = randomUUID();

        connection.send({ type: "retry", id, to: mayor });

        expect(await connection.next()).toEqual({
            type: "fault",
            text: "This server does not take `retry` messages.",
            to: id,
        });
    });

    it("answers a second hello with a fault, and keeps the connection open", async () => {
        const server = await start({ say: heard });
        const connection = await join(server);
        const said = say("Again.");

        connection.send(hello);
        connection.send(said);

        expect(await connection.next()).toEqual({
            type: "fault",
            text: "This connection said hello already.",
        });
        expect(await connection.next()).toEqual(heard(said));
    });

    it("handles what a client sent just before it went (S1)", async () => {
        const said: Say[] = [];
        const server = await start({
            hello: async () => {
                await sleep(20);
                return state();
            },
            say: (message) => {
                said.push(message);
                return undefined;
            },
        });
        const connection = await open(server);
        const line = say("Carry on without me.");

        connection.send(hello);
        connection.send(line);
        await connection.close();

        await vi.waitFor(() => expect(said).toEqual([line]));
    });

    it("closes a connection whose hello cannot be answered, with a fault", async () => {
        const server = await start({
            hello: () => {
                throw new Error("The home is closing.");
            },
        });
        const connection = await open(server);

        connection.send(hello);

        expect(await connection.next()).toEqual({
            type: "fault",
            text: "The home is closing.",
        });
        expect(await connection.closed).toBe(1011);
    });

    it("closes a connection whose state cannot be sent, with a fault, and serves the others", async () => {
        const unsendable = {
            ...state(),
            toJSON: () => {
                throw new Error("The state cannot be written.");
            },
        };
        let first = true;
        const server = await start({
            hello: () => {
                const answer = first ? unsendable : state();
                first = false;
                return answer;
            },
        });
        const connection = await open(server);

        connection.send(hello);

        expect(await connection.next()).toEqual({
            type: "fault",
            text: "The state cannot be written.",
        });
        expect(await connection.closed).toBe(1011);
        await join(server);
    });

    it("aborts a client's signal once its connection closes", async () => {
        const signals: AbortSignal[] = [];
        const server = await start({
            say: (message, client) => {
                signals.push(client.signal);
                return heard(message);
            },
        });
        const connection = await join(server);
        connection.send(say("Bye."));
        await connection.next();

        expect(signals.map((signal) => signal.aborted)).toEqual([false]);
        await connection.close();

        await vi.waitFor(() => expect(signals[0]?.aborted).toBe(true));
    });

    it("closes every connection when it closes", async () => {
        const server = await start();
        const connection = await join(server);

        await server.close();

        expect(await connection.closed).toBe(1001);
        await expect(connection.next()).rejects.toThrow(
            "The connection to the server closed (1001).",
        );
    });

    it("closes without waiting long on a client that does not answer its close", async () => {
        const server = await start();
        const stalled = new WebSocket(server.url);
        onTestFinished(() => stalled.terminate());
        await once(stalled, "open");
        // As a `hamlet chat` suspended with Ctrl-Z: nothing it was sent is read.
        stalled.pause();

        const began = performance.now();
        await server.close();

        // ws waits 30 s for a close to be answered.
        expect(performance.now() - began).toBeLessThan(5000);
    }, 40_000);
});

describe("the client's end of a connection", () => {
    it("refuses a message from the server that does not fit protocol v0, naming it", async () => {
        const server = await start({
            hello: () => ({ ...state(), revision: 1.5 }),
        });
        const connection = await open(server);

        connection.send(hello);

        await expect(connection.next()).rejects.toThrow(
            "This `state` message does not fit protocol v0: `revision` must be integer.",
        );
    });

    it("fails to connect where no server listens", async () => {
        const server = await start();
        await server.close();

        await expect(connect(server.url)).rejects.toThrow("ECONNREFUSED");
    });
});
