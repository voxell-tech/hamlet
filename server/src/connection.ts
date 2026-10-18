import { createHash, timingSafeEqual } from "node:crypto";
import { once } from "node:events";
import { WebSocket, WebSocketServer } from "ws";
import {
    type ClientMessage,
    type Fault,
    type Hello,
    type Malformed,
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
    /** Reports a detached handler's defect through the same path as a thrown handler. */
    fail(error: unknown, to?: string): void;
    /** Aborted once the connection closes. */
    readonly signal: AbortSignal;
};

/** A transport rejection or an unexpected failure while serving a connection. */
export type ConnectionFailure =
    | { kind: "connection"; text: string }
    | { kind: "defect"; cause: unknown };

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
    /** Renders connection failures with the home's credential scrubbing. */
    fault?: (
        to: string | undefined,
        failure: ConnectionFailure | Malformed,
    ) => Fault;
    /** Records the cause of a defect, with its request id when available. */
    report?: (error: unknown, to?: string) => void;
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
        accept(
            socket,
            options.token,
            options.handlers,
            members,
            helloMs,
            options.fault,
            options.report,
        ),
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
    render: (
        to: string | undefined,
        failure: ConnectionFailure | Malformed,
    ) => Fault = (to, failure) =>
        fault(
            failure.kind === "malformed"
                ? failure.text
                : connectionFailureText(failure),
            to,
        ),
    report: (error: unknown, to?: string) => void = (error, to) =>
        console.error(
            `Request ${to ?? "(connection)"} failed: ${connectionFailureText({ kind: "defect", cause: error })}`,
        ),
): void {
    const closed = new AbortController();
    const client: Client = {
        send: (message) => send(socket, message),
        signal: closed.signal,
        fail: (error, to) => {
            report(error, to);
            send(socket, render(to, { kind: "defect", cause: error }));
        },
    };
    let welcomed = false;
    let refused = false;
    let handled = Promise.resolve();
    const late = setTimeout(
        () => refuse({ kind: "connection", text: BEGIN }, REFUSED),
        helloMs,
    );
    socket.on("message", (data) => {
        // A failure outside a request's handler, such as a state that cannot
        // be sent, ends the connection.
        handled = handled
            .then(() => receive(String(data)))
            .catch((error: unknown) => {
                report(error);
                refuse({ kind: "defect", cause: error }, FAILED);
            });
    });
    // An error is followed by the close.
    socket.on("error", () => {});
    socket.on("close", () => {
        clearTimeout(late);
        members.delete(socket);
        closed.abort();
    });

    // Anything the client sent after the refused message is dropped.
    function refuse(
        failure: ConnectionFailure | Malformed,
        code: number,
    ): void {
        refused = true;
        clearTimeout(late);
        members.delete(socket);
        const message = render(undefined, failure);
        send(socket, message);
        socket.close(code, code === TOO_LARGE ? message.text : undefined);
    }

    async function receive(text: string): Promise<void> {
        if (refused) return;
        const parsed = parseClient(text);
        if (!welcomed) {
            if (!parsed.ok) return refuse(parsed.error, REFUSED);
            if (parsed.value.type !== "hello")
                return refuse({ kind: "connection", text: BEGIN }, REFUSED);
            if (!same(parsed.value.token, token)) {
                return refuse(
                    {
                        kind: "connection",
                        text: "That is not this server's token.",
                    },
                    REFUSED,
                );
            }
            clearTimeout(late);
            return welcome(parsed.value);
        }
        if (!parsed.ok) {
            return send(socket, render(parsed.error.to, parsed.error));
        }
        const message = parsed.value;
        if (message.type === "hello") {
            return send(
                socket,
                render(undefined, {
                    kind: "connection",
                    text: "This connection said hello already.",
                }),
            );
        }
        const id = "id" in message ? message.id : undefined;
        // SAFETY: the same parsed discriminant selects the handler and its message; TypeScript loses that correlation through indexed mapped types.
        // Verified by: connection.test.ts handles a client's messages one at a time, in the order they came.
        const handler = handlers[message.type] as
            | ((message: Request, client: Client) => Answered<ServerMessage>)
            | undefined;
        if (handler === undefined) {
            const text = `This server does not take \`${message.type}\` messages.`;
            return send(socket, render(id, { kind: "connection", text }));
        }
        try {
            const answer = await handler(message, client);
            if (answer !== undefined) send(socket, answer);
        } catch (error) {
            client.fail(error, id);
        }
    }

    async function welcome(hello: Hello): Promise<void> {
        const held: ServerMessage[] = [];
        if (!closed.signal.aborted) members.set(socket, held);
        const state = await handlers.hello(hello, client);
        if (state !== undefined) send(socket, state);
        for (const message of held) {
            if (
                state !== undefined &&
                (message.type === "state" || message.type === "change") &&
                message.epoch === state.epoch &&
                message.revision <= state.revision
            )
                continue;
            send(socket, message);
        }
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

/** Public wording for a transport rejection or a defect; credential scrubbing belongs to the envelope. */
export function connectionFailureText(failure: ConnectionFailure): string {
    switch (failure.kind) {
        case "connection":
            return failure.text;
        case "defect":
            return failure.cause instanceof Error
                ? failure.cause.message
                : String(failure.cause);
    }
    const unhandled: never = failure;
    throw new Error(`Unclassified connection failure: ${unhandled}`);
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

/** Opens a connection whose first message must be hello; aborting releases its socket. */
export async function connect(
    url: string,
    signal?: AbortSignal,
): Promise<Connection> {
    signal?.throwIfAborted();
    const socket = new WebSocket(url);
    const abort = () => socket.terminate();
    signal?.addEventListener("abort", abort, { once: true });
    socket.once("close", () => signal?.removeEventListener("abort", abort));
    socket.on("error", () => {});
    await once(socket, "open");
    const arrived: (ServerMessage | Error)[] = [];
    const waiting: ((item: ServerMessage | Error) => void)[] = [];
    let ended: Error | undefined;
    socket.on("message", (data) => {
        const parsed = parseServer(String(data));
        const item = parsed.ok ? parsed.value : new Error(parsed.error.text);
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
