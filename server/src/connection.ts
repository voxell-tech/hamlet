import { createHash, timingSafeEqual } from "node:crypto";
import { once } from "node:events";
import { WebSocket, WebSocketServer } from "ws";
import {
    type ClientMessage,
    type Fault,
    type Hello,
    parseClient,
    parseServer,
    type ServerMessage,
    type State,
} from "./protocol.ts";

/** A message a client sends after its hello. */
export type Request = Exclude<ClientMessage, Hello>;

/** A connection at the server's end. */
export type Client = {
    send(message: ServerMessage): void;
    /** Aborted once the connection closes. */
    readonly signal: AbortSignal;
};

type Answered<M> = M | undefined | Promise<M | undefined>;

/** Handlers of the messages clients send, each answering with a message for its client or nothing, and each client's messages handled one at a time in the order they came. */
export type Handlers = {
    /** The state a client starts from, or nothing when the state it saw is current. */
    hello(message: Hello, client: Client): Answered<State>;
} & {
    [T in Request["type"]]?: (
        message: Extract<Request, { type: T }>,
        client: Client,
    ) => Answered<ServerMessage>;
};

/** A server of protocol v0. */
export type Server = {
    /** The address clients connect to. */
    readonly url: string;
    /** Sends `message` to every client that has said hello. */
    broadcast(message: ServerMessage): void;
    /** Closes every connection and stops listening. */
    close(): Promise<void>;
};

// Close codes of RFC 6455.
const GOING = 1001;
const REFUSED = 1008;
const FAILED = 1011;
const TOO_LARGE = 1009;

/** The wire-byte limit for every message, including hello. */
const LARGEST = 1024 * 1024;

/** Time a connection has to say hello, unless the server is given its own. */
const HELLO_MS = 10_000;

/** Time a client has to answer the server's close before it is cut off. */
const GRACE_MS = 1000;

const BEGIN = "A connection begins with hello and the home's token.";

class CappedSocket extends WebSocket {
    override close(code?: number, data?: string | Buffer): void {
        if (
            code === TOO_LARGE &&
            this.readyState === WebSocket.OPEN &&
            data === undefined
        )
            this.send(
                JSON.stringify(
                    fault(
                        `A message is larger than the ${LARGEST} bytes this connection takes.`,
                    ),
                ),
            );
        super.close(
            code,
            data ?? (code === TOO_LARGE ? "Message too large." : undefined),
        );
    }
}

/** Serves protocol v0 on 127.0.0.1 at `port`, to clients that say hello with `token` (S4). */
export async function serve(options: {
    token: string;
    port: number;
    handlers: Handlers;
    /** Time a connection has to say hello. */
    helloMs?: number;
}): Promise<Server> {
    const sockets = new WebSocketServer({
        host: "127.0.0.1",
        port: options.port,
        maxPayload: LARGEST,
        WebSocket: CappedSocket,
    });
    await once(sockets, "listening");
    const address = sockets.address();
    if (address === null || typeof address === "string") {
        throw new Error("The server listens on no port.");
    }
    // Each client that said hello, with what was broadcast while its state
    // was made, which follows the state.
    const members = new Map<WebSocket, ServerMessage[] | undefined>();
    const helloMs = options.helloMs ?? HELLO_MS;
    sockets.on("connection", (socket) =>
        accept(socket, options.token, options.handlers, members, helloMs),
    );
    let closing: Promise<void> | undefined;
    return {
        url: `ws://127.0.0.1:${address.port}`,
        broadcast: (message) => {
            for (const [socket, held] of members) {
                if (held === undefined) send(socket, message);
                else held.push(message);
            }
        },
        close: () => {
            closing ??= new Promise<void>((resolve, reject) => {
                for (const socket of sockets.clients) socket.close(GOING);
                const cut = setTimeout(() => {
                    for (const socket of sockets.clients) socket.terminate();
                }, GRACE_MS);
                sockets.close((error) => {
                    clearTimeout(cut);
                    if (error) reject(error);
                    else resolve();
                });
            });
            return closing;
        },
    };
}

function accept(
    socket: WebSocket,
    token: string,
    handlers: Handlers,
    members: Map<WebSocket, ServerMessage[] | undefined>,
    helloMs: number,
): void {
    const closed = new AbortController();
    const client: Client = {
        send: (message) => send(socket, message),
        signal: closed.signal,
    };
    let welcomed = false;
    let refused = false;
    let handled = Promise.resolve();
    const late = setTimeout(() => refuse(BEGIN, REFUSED), helloMs);
    socket.on("message", (data) => {
        // A failure outside a request's handler, such as a state that cannot
        // be sent, ends the connection.
        handled = handled
            .then(() => receive(String(data)))
            .catch((error: unknown) => refuse(reason(error), FAILED));
    });
    // An error is followed by the close.
    socket.on("error", () => {});
    socket.on("close", () => {
        clearTimeout(late);
        members.delete(socket);
        closed.abort();
    });

    // Anything the client sent after the refused message is dropped.
    function refuse(text: string, code: number): void {
        refused = true;
        clearTimeout(late);
        members.delete(socket);
        send(socket, fault(text));
        socket.close(code, code === TOO_LARGE ? text : undefined);
    }

    async function receive(text: string): Promise<void> {
        if (refused) return;
        const parsed = parseClient(text);
        if (!welcomed) {
            if ("malformed" in parsed) return refuse(parsed.malformed, REFUSED);
            if (parsed.message.type !== "hello") return refuse(BEGIN, REFUSED);
            if (!same(parsed.message.token, token)) {
                return refuse("That is not this server's token.", REFUSED);
            }
            clearTimeout(late);
            return welcome(parsed.message);
        }
        if ("malformed" in parsed) {
            return send(socket, fault(parsed.malformed, parsed.to));
        }
        const message = parsed.message;
        if (message.type === "hello") {
            return send(socket, fault("This connection said hello already."));
        }
        const id = "id" in message ? message.id : undefined;
        const handler = handlers[message.type] as
            | ((message: Request, client: Client) => Answered<ServerMessage>)
            | undefined;
        if (handler === undefined) {
            const text = `This server does not take \`${message.type}\` messages.`;
            return send(socket, fault(text, id));
        }
        try {
            const answer = await handler(message, client);
            if (answer !== undefined) send(socket, answer);
        } catch (error) {
            send(socket, fault(reason(error), id));
        }
    }

    async function welcome(hello: Hello): Promise<void> {
        const held: ServerMessage[] = [];
        if (!closed.signal.aborted) members.set(socket, held);
        const state = await handlers.hello(hello, client);
        if (state !== undefined) send(socket, state);
        for (const message of held) send(socket, message);
        welcomed = true;
        if (members.has(socket)) members.set(socket, undefined);
    }
}

function send(socket: WebSocket, message: ServerMessage): void {
    socket.send(JSON.stringify(message));
}

/** A fault in `text`, answering the request `to` when there is one. */
function fault(text: string, to?: string): Fault {
    return to === undefined
        ? { type: "fault", text }
        : { type: "fault", text, to };
}

function reason(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
}

/** True when `given` is `token`, taking the same time either way. */
function same(given: string, token: string): boolean {
    const digest = (text: string) => createHash("sha256").update(text).digest();
    return timingSafeEqual(digest(given), digest(token));
}

/** A client's connection to a server of protocol v0. */
export type Connection = {
    send(message: ClientMessage): void;
    /** The next message from the server, in the order they came. */
    next(): Promise<ServerMessage>;
    /** The close code, once the connection has closed. */
    readonly closed: Promise<number>;
    close(): Promise<void>;
};

/** Opens a connection to the server at `url`, whose first message must be hello. */
export async function connect(url: string): Promise<Connection> {
    const socket = new WebSocket(url);
    socket.on("error", () => {});
    await once(socket, "open");
    const arrived: (ServerMessage | Error)[] = [];
    const waiting: ((item: ServerMessage | Error) => void)[] = [];
    let ended: Error | undefined;
    socket.on("message", (data) => {
        const parsed = parseServer(String(data));
        const item =
            "message" in parsed ? parsed.message : new Error(parsed.malformed);
        const wait = waiting.shift();
        if (wait === undefined) arrived.push(item);
        else wait(item);
    });
    const closed = new Promise<number>((resolve) => {
        socket.on("close", (code) => {
            ended = new Error(`The connection to the server closed (${code}).`);
            for (const wait of waiting.splice(0)) wait(ended);
            resolve(code);
        });
    });
    return {
        send: (message) => socket.send(JSON.stringify(message)),
        next: async () => {
            const item =
                arrived.shift() ??
                ended ??
                (await new Promise<ServerMessage | Error>((resolve) =>
                    waiting.push(resolve),
                ));
            if (item instanceof Error) throw item;
            return item;
        },
        closed,
        close: async () => {
            socket.close();
            await closed;
        },
    };
}
