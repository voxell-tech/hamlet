import { randomUUID } from "node:crypto";
import { expect, vi } from "vitest";
import { type Connection, connect } from "../src/connection.ts";
import type { HomeOptions } from "../src/home.ts";
import type {
    ClientMessage,
    Item,
    Op,
    ServerMessage,
} from "../src/protocol.ts";
import { serveHome } from "../src/server.ts";
import { closeAfter, context, scratch, testHome } from "./support.ts";

export const mayor = { hamlet: "home", agent: "mayor" };
export const chat = { with: mayor };

/** A served scratch home and its authenticated WebSocket client. */
export async function served(options: HomeOptions) {
    const home = await testHome(scratch(), options);
    const server = await serveHome(home, 0, context);
    closeAfter(() => server.close());
    const client = await joined(server.url, home.credentials.token);
    return { home, server, ...client };
}

export async function joined(url: string, token: string) {
    const connection = await connect(url);
    closeAfter(() => connection.close());
    connection.send({ type: "hello", v: 0, token });
    const state = await connection.next();
    if (state.type !== "state") throw new Error("No state after hello");
    const messages: ServerMessage[] = [state];
    const errors: unknown[] = [];
    void (async () => {
        try {
            for (;;) messages.push(await connection.next());
        } catch (error) {
            errors.push(error);
        }
    })();
    return { connection, state, messages, errors };
}

export async function received<M extends ServerMessage>(
    messages: ServerMessage[],
    matches: (message: ServerMessage) => message is M,
): Promise<M> {
    let found: M | undefined;
    await vi.waitFor(
        () => {
            found = messages.find(matches);
            expect(found).toBeDefined();
        },
        { timeout: 10_000 },
    );
    return found as M;
}

export async function request(
    connection: Connection,
    messages: ServerMessage[],
    message: ClientMessage & { id: string },
) {
    connection.send(message);
    return received(
        messages,
        (answer): answer is ServerMessage =>
            "to" in answer && answer.to === message.id,
    );
}

export function changes(messages: ServerMessage[]) {
    return messages.flatMap((message) =>
        message.type === "change" ? message.changes : [],
    );
}

export function agents(messages: ServerMessage[]) {
    return changes(messages).flatMap((change) =>
        change.type === "agent" ? [change.agent] : [],
    );
}

export function say(connection: Connection, text: string) {
    connection.send({ type: "say", id: randomUUID(), to: mayor, text });
}

/** A turn op applied as a client does. */
export function fold(
    items: Item[],
    status: { working: boolean; pending: number },
    op: Op,
): void {
    const find = (id: string) => {
        const item = items.find((each) => each.id === id);
        if (item === undefined) throw new Error(`No item ${id}`);
        return item;
    };
    switch (op.op) {
        case "add":
            if (op.replaces !== undefined) {
                items.splice(items.indexOf(find(op.replaces)), 1);
            }
            items.push(structuredClone(op.item));
            return;
        case "append":
            find(op.item).text += op.text;
            return;
        case "close": {
            const { tool } = find(op.item);
            if (tool === undefined) throw new Error(`${op.item} is no tool`);
            Object.assign(tool, {
                exit: op.exit,
                ms: op.ms,
                failure: op.failure,
                ...(op.change === undefined ? {} : { change: op.change }),
            });
            return;
        }
        case "status":
            Object.assign(status, {
                working: op.working,
                pending: op.pending,
            });
            return;
        case "reset":
            items.length = 0;
            return;
        case "usage":
            return;
    }
}
