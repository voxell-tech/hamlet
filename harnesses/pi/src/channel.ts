import { createInterface } from "node:readline";
import type { Readable, Writable } from "node:stream";
import {
    LINK_VERSION,
    parseServerLine,
    type RuntimeMessage,
    type ServerMessage,
} from "./link.ts";

/** A message the runtime sends, before the link adds its version and id. */
export type Outgoing = RuntimeMessage extends infer M
    ? M extends RuntimeMessage
        ? Omit<M, "v" | "id">
        : never
    : never;

type RequestType =
    | "provider_request"
    | "tools"
    | "create"
    | "submit"
    | "write"
    | "configure"
    | "fork"
    | "cancel"
    | "history"
    | "close"
    | "status"
    | "answered";

/** A server message that asks something of the runtime or tells it something. */
export type ServerRequest = Extract<ServerMessage, { type: RequestType }>;

/** A server message that answers one of the runtime's requests. */
export type ServerReply = Exclude<ServerMessage, { type: RequestType }>;

/** The runtime's end of the harness link: one JSON message per line each way. */
export class Channel {
    readonly #output: Writable;
    readonly #waiting = new Map<string, (reply: ServerReply) => void>();
    #next = 1;
    #closed = false;
    #onClose: () => void = () => {};

    constructor(input: Readable, output: Writable) {
        this.#output = output;
        const lines = createInterface({ input, crlfDelay: Infinity });
        lines.on("line", (line) => this.#receive(line));
        lines.on("close", () => {
            this.#closed = true;
            this.#onClose();
        });
    }

    /** Requests that arrived before `serve`, handled once it is called. */
    #early: ServerRequest[] = [];
    #handle: ((request: ServerRequest) => void) | undefined;

    /** Sets what handles the server's requests, and what runs once the server is gone. */
    serve(handle: (request: ServerRequest) => void, onClose: () => void): void {
        this.#handle = handle;
        this.#onClose = onClose;
        for (const request of this.#early.splice(0)) handle(request);
        if (this.#closed) onClose();
    }

    /** Sends `message` and returns its id. */
    send(message: Outgoing): string {
        const id = `r${this.#next++}`;
        this.#output.write(
            `${JSON.stringify({ ...message, v: LINK_VERSION, id })}\n`,
        );
        return id;
    }

    /** Sends `message` and waits for the server's reply to it. */
    request(message: Outgoing): Promise<ServerReply> {
        return new Promise((resolve) => {
            this.#waiting.set(this.send(message), resolve);
        });
    }

    #receive(line: string): void {
        const parsed = parseServerLine(line);
        if (!parsed.ok) {
            this.send({
                type: "error",
                code: "malformed",
                message: parsed.error,
                ...readableId(line),
            });
            return;
        }
        const sorted = sort(parsed.value);
        if ("request" in sorted) {
            if (this.#handle === undefined) this.#early.push(sorted.request);
            else this.#handle(sorted.request);
            return;
        }
        const { re } = sorted.reply;
        const waiting = re === undefined ? undefined : this.#waiting.get(re);
        if (re === undefined || waiting === undefined) {
            // A server's error about a message this runtime did not wait on has nobody to tell but the log.
            process.stderr.write(`link: ${JSON.stringify(sorted.reply)}\n`);
            return;
        }
        this.#waiting.delete(re);
        waiting(sorted.reply);
    }
}

function sort(
    message: ServerMessage,
): { request: ServerRequest } | { reply: ServerReply } {
    switch (message.type) {
        case "provider_request":
        case "tools":
        case "create":
        case "submit":
        case "write":
        case "configure":
        case "fork":
        case "cancel":
        case "history":
        case "close":
        case "status":
        case "answered":
            return { request: message };
        case "welcome":
        case "refused":
        case "allowed":
        case "waiting":
        case "refusal":
        case "result":
        case "allowance":
        case "stopped":
        case "charged":
        case "scrubbed":
        case "sections":
        case "verdict":
        case "error":
            return { reply: message };
        default: {
            const unhandled: never = message;
            throw new Error(`Unsorted link message ${String(unhandled)}`);
        }
    }
}

function readableId(line: string): { re?: string } {
    try {
        const value: unknown = JSON.parse(line);
        if (
            typeof value === "object" &&
            value !== null &&
            "id" in value &&
            typeof value.id === "string"
        )
            return { re: value.id };
    } catch {}
    return {};
}
