import { randomUUID } from "node:crypto";
import { createInterface } from "node:readline";
import type { Readable, Writable } from "node:stream";
import { COMMANDS } from "../commands.ts";
import { strip } from "../oversight/words.ts";
import {
    type Address,
    applyOp,
    type ChatRef,
    type Item,
    type Op,
    type ServerMessage,
    type State,
    type Wait,
} from "../protocol.ts";
import { findInState } from "../realm/roads.ts";
import { connectChief } from "./client.ts";

/** A terminal chat over the protocol, with answers on `out` and everything else on `err` (S13, C30). */
export async function chat(options: {
    url: string;
    token: string;
    to: Address;
    input: Readable;
    out: Writable;
    err: Writable;
}): Promise<void> {
    const { connection, state: welcome } = await connectChief(options);
    let input: ReturnType<typeof createInterface> | undefined;
    let reading = true;
    try {
        const screen = new Screen(options.out, options.err);
        screen.show(welcome);
        options.err.write(
            `hamlet: Ctrl-D quits after your lines are answered; Ctrl-C quits at once and stops no agent\n${COMMANDS.filter(
                ({ deed }) => !["go", "withdraw", "reply"].includes(deed),
            )
                .map(
                    ({ name, args, description }) =>
                        `  ${[name, args].filter(Boolean).join(" ")}: ${description}`,
                )
                .join(
                    "\n",
                )}\n  /agent <name>, /hamlet <name>: Switch who you talk to\n`,
        );
        let state: State = welcome;
        if (state.settings.unset)
            options.err.write(
                "Set up a provider with `hamlet login <provider> [--key]`, then try a hello with `hamlet models try`. Run these through `npm run hamlet --` for now.\n",
            );
        const resolve = (to: Address): Address => {
            if (!state.hamlets.some((hamlet) => hamlet.id === to.hamlet))
                throw new Error(`no hamlet named \`${to.hamlet}\``);
            const ref = findInState(state, to);
            const agent = state.agents.find(
                (agent) =>
                    agent.ref.hamlet === ref?.hamlet &&
                    agent.ref.number === ref.number,
            );
            if (agent === undefined)
                throw new Error(`no agent named \`${to.agent}\``);
            return {
                hamlet: to.hamlet,
                agent: agent.mayor ? "mayor" : agent.name,
            };
        };
        let target = resolve(options.to);
        const chat = { with: target };
        const opens = new Set<string>();
        const numbers = new Map<string, number>();
        const pinned = new Set<string>();
        const open = (id: string, ref: ChatRef = chat) => {
            opens.add(id);
            connection.send({ type: "open", id, chat: ref });
        };
        open(randomUUID());
        // The EOF barrier follows admission of every line sent before it.
        let last: string | undefined;
        let final: string | undefined;
        const pending = new Set<string>();
        const interactive =
            "isTTY" in options.input && options.input.isTTY === true;
        let prompted = false;
        const prompt = () => {
            if (!interactive || last !== undefined) return;
            screen.end();
            options.err.write("\n> ");
        };
        input = createInterface({ input: options.input });
        const lines = input;
        void (async () => {
            for await (const line of lines) {
                if (!reading) break;
                if (line.trim() === "") {
                    prompt();
                    continue;
                }
                const switched = /^(\/agent|\/hamlet)(?:\s+([\s\S]*))?$/.exec(
                    line.trim(),
                );
                if (switched !== null) {
                    const name = switched[2]?.trim();
                    const kind = switched[1] === "/agent" ? "agent" : "hamlet";
                    try {
                        if (!name) throw new Error(`usage: /${kind} <name>`);
                        const hamlet =
                            kind === "hamlet"
                                ? state.hamlets.find(
                                      (hamlet) =>
                                          hamlet.id === name ||
                                          hamlet.name === name,
                                  )
                                : undefined;
                        if (kind === "hamlet" && hamlet === undefined)
                            throw new Error(`no hamlet named \`${name}\``);
                        const ref =
                            kind === "agent"
                                ? findInState(state, {
                                      agent: name,
                                      ...(name === "mayor"
                                          ? { hamlet: target.hamlet }
                                          : {}),
                                  })
                                : undefined;
                        const agent = state.agents.find(
                            (agent) =>
                                agent.ref.hamlet === ref?.hamlet &&
                                agent.ref.number === ref.number,
                        );
                        if (kind === "agent" && agent === undefined)
                            throw new Error(`no agent named \`${name}\``);
                        const next = resolve(
                            agent
                                ? {
                                      hamlet: agent.ref.hamlet,
                                      agent: agent.mayor ? "mayor" : agent.name,
                                  }
                                : {
                                      hamlet: hamlet?.id ?? name,
                                      agent: "mayor",
                                  },
                        );
                        if (JSON.stringify(next) !== JSON.stringify(target)) {
                            target = next;
                            chat.with = target;
                            open(randomUUID());
                        }
                        options.err.write(
                            `  (talking to the ${kind === "agent" ? `agent ${name}` : `mayor of ${name}`})\n`,
                        );
                    } catch (error) {
                        options.err.write(
                            `error: ${error instanceof Error ? error.message : String(error)}\n`,
                        );
                    }
                    prompt();
                    continue;
                }
                const id = randomUUID();
                pending.add(id);
                connection.send({
                    type: "say",
                    id,
                    to: target,
                    text: line,
                });
            }
            if (reading) {
                last = randomUUID();
                open(last);
            }
        })().catch(() => connection.close());
        let ending = false;
        for (;;) {
            const message = await connection.next();
            if (
                message.type === "fault" &&
                message.to !== undefined &&
                opens.has(message.to)
            )
                throw new Error(message.text);
            if (message.type === "transcript" && message.to !== undefined)
                opens.delete(message.to);
            if (message.type === "state") state = message;
            if (message.type === "change") {
                for (const change of message.changes) {
                    if (change.type === "hamlet")
                        state.hamlets = [
                            ...state.hamlets.filter(
                                (hamlet) => hamlet.id !== change.hamlet.id,
                            ),
                            change.hamlet,
                        ];
                    if (change.type === "agent")
                        state.agents = [
                            ...state.agents.filter(
                                (agent) =>
                                    agent.ref.hamlet !==
                                        change.agent.ref.hamlet ||
                                    agent.ref.number !==
                                        change.agent.ref.number,
                            ),
                            change.agent,
                        ];
                    if (change.type === "agent-gone")
                        state.agents = state.agents.filter(
                            (agent) =>
                                agent.ref.hamlet !== change.ref.hamlet ||
                                agent.ref.number !== change.ref.number,
                        );
                }
            }
            if (
                message.type === "transcript" &&
                message.chat.past === undefined &&
                message.number !== undefined
            ) {
                const key = JSON.stringify(message.chat.with);
                const previous = numbers.get(key);
                numbers.set(key, message.number);
                if (previous !== undefined && previous !== message.number) {
                    const ref = { with: message.chat.with, past: previous };
                    const pin = JSON.stringify(ref);
                    if (!pinned.has(pin)) {
                        pinned.add(pin);
                        open(randomUUID(), ref);
                    }
                }
            }
            if (
                !(
                    (message.type === "transcript" ||
                        message.type === "turn") &&
                    message.chat.past !== undefined &&
                    message.chat.past ===
                        numbers.get(JSON.stringify(message.chat.with))
                )
            )
                screen.show(message);
            if (message.type === "transcript" && !prompted) {
                prompted = true;
                prompt();
            }
            if (
                message.type === "transcript" &&
                last !== undefined &&
                message.to === last
            ) {
                ending = true;
            }
            if (
                (message.type === "reply" || message.type === "fault") &&
                message.to !== undefined
            ) {
                if (pending.delete(message.to)) prompt();
            }
            if (ending && pending.size === 0 && final === undefined) {
                final = randomUUID();
                open(final);
            }
            if (
                message.type === "transcript" &&
                final !== undefined &&
                message.to === final
            )
                break;
        }
        screen.end();
    } finally {
        reading = false;
        input?.close();
        options.input.pause();
        await connection.close();
    }
}

/** A chat's printer, after the REPL's (terminal.rs:153-237). */
class Screen {
    readonly #out: Writable;
    readonly #err: Writable;
    /** Each item that streams: the characters printed of it, and whether it is reasoning. */
    readonly #streamed = new Map<
        string,
        { printed: number; reasoning: boolean }
    >();
    /** The item whose line is unfinished, and the stream it is on. */
    #open: { id: string; stream: Writable } | undefined;
    readonly #waits = new Set<string>();
    readonly #seen = new Set<string>();
    #opened = false;
    readonly #views = new Map<
        string,
        { items: Item[]; working: boolean; pending: number }
    >();

    constructor(out: Writable, err: Writable) {
        this.#out = out;
        this.#err = err;
    }

    show(message: ServerMessage): void {
        if (message.type === "state")
            for (const wait of message.waits) this.#wait(wait);
        if (message.type === "change")
            for (const change of message.changes)
                if (change.type === "wait") this.#wait(change.wait);
        if (message.type === "fault") this.#line(`\nerror: ${message.text}`);
        if (message.type === "reply" && message.text !== "")
            this.#line(
                message.kind === "error"
                    ? `\nerror: ${message.text}`
                    : `\n  (${message.text})`,
            );
        if (message.type === "transcript") {
            const items = message.items.map((item) => ({
                ...item,
                id: this.#id(message, item.id),
            }));
            if (!this.#opened) {
                const last = items.findLast(
                    (item) => item.kind === "assistant",
                );
                for (const item of items)
                    if (
                        item !== last &&
                        !(item.kind === "tool" && item.tool?.exit === null)
                    )
                        this.#seen.add(item.id);
                this.#opened = true;
            }
            const view = this.#view(message);
            view.items.splice(0, view.items.length, ...items);
            view.working = message.working;
            view.pending = message.pending;
            for (const item of view.items) this.#add(item, undefined);
            if (!message.working) this.end();
        }
        if (message.type === "turn") {
            const view = this.#view(message);
            for (const op of message.ops) {
                if (op.op === "add")
                    this.#op(view, {
                        ...op,
                        item: { ...op.item, id: this.#id(message, op.item.id) },
                        ...(op.replaces === undefined
                            ? {}
                            : { replaces: this.#id(message, op.replaces) }),
                    });
                else if (op.op === "append" || op.op === "close")
                    this.#op(view, { ...op, item: this.#id(message, op.item) });
                else this.#op(view, op);
            }
        }
    }

    #id(
        message: Extract<ServerMessage, { type: "turn" | "transcript" }>,
        id: string,
    ): string {
        return id.startsWith("live:")
            ? `${JSON.stringify(message.chat.with)}:${message.number ?? message.chat.past}:${id}`
            : id;
    }

    #wait(wait: Wait): void {
        if (this.#waits.has(wait.id)) return;
        this.#waits.add(wait.id);
        this.#line(`\n  NEEDS YOU: ${strip(wait)}`);
    }

    /** The end of the unfinished output line. */
    end(): void {
        this.#open?.stream.write("\n");
        this.#open = undefined;
    }

    #view(message: Extract<ServerMessage, { type: "turn" | "transcript" }>) {
        const key = `${JSON.stringify(message.chat)}:${message.number ?? message.chat.past}`;
        let view = this.#views.get(key);
        if (view === undefined) {
            view = { items: [], working: false, pending: 0 };
            this.#views.set(key, view);
        }
        return view;
    }

    #op(
        view: { items: Item[]; working: boolean; pending: number },
        op: Op,
    ): void {
        if (op.op === "append" || op.op === "close") {
            const item = view.items.find((item) => item.id === op.item);
            if (
                item === undefined ||
                (op.op === "close" && item.tool === undefined)
            )
                return;
        }
        // A page or a temporarily hidden past view may lack the replaced live item.
        if (
            op.op === "add" &&
            op.replaces !== undefined &&
            !view.items.some((item) => item.id === op.replaces)
        )
            applyOp(view.items, view, { op: "add", item: op.item });
        else applyOp(view.items, view, op);
        if (op.op === "add") this.#add(op.item, op.replaces);
        if (op.op === "append") {
            const item = view.items.find((item) => item.id === op.item);
            const printed = this.#streamed.get(op.item)?.printed ?? 0;
            if (item !== undefined)
                this.#stream(op.item, item.text.slice(printed));
        }
        if (op.op === "close" && op.exit !== 0)
            this.#line(`  [exit ${op.exit}]`);
        if (op.op === "status" && !view.working) this.end();
    }

    #add(item: Item, replaces: string | undefined): void {
        if (this.#seen.has(item.id)) {
            const printed = this.#streamed.get(item.id)?.printed;
            if (printed !== undefined)
                this.#stream(item.id, item.text.slice(printed));
            return;
        }
        this.#seen.add(item.id);
        if (replaces !== undefined) this.#seen.delete(replaces);
        switch (item.kind) {
            case "assistant":
            case "reasoning": {
                const replaced =
                    replaces === undefined
                        ? undefined
                        : this.#streamed.get(replaces);
                const printed = replaced?.printed ?? 0;
                if (replaces !== undefined) this.#streamed.delete(replaces);
                this.#streamed.set(item.id, {
                    printed,
                    reasoning: item.kind === "reasoning",
                });
                const open = this.#open;
                if (open !== undefined && open.id === replaces) {
                    open.id = item.id;
                }
                this.#stream(item.id, item.text.slice(printed));
                break;
            }
            case "tool":
                this.#line(`\n  [${item.tool?.name}] ${item.tool?.summary}`);
                break;
            case "notice":
                this.#line(`\n  (${item.text})`);
                break;
            case "error":
                this.#line(`\nerror: ${item.text}`);
                break;
        }
    }

    /** More of an answer on stdout, or dimmed reasoning on stderr. */
    #stream(id: string, text: string): void {
        const streamed = this.#streamed.get(id);
        if (streamed === undefined || text === "") return;
        streamed.printed += text.length;
        const stream = streamed.reasoning ? this.#err : this.#out;
        if (this.#open?.id !== id) this.end();
        stream.write(streamed.reasoning ? `\x1b[2m${text}\x1b[0m` : text);
        this.#open = { id, stream };
    }

    /** A separate line on stderr. */
    #line(text: string): void {
        this.end();
        this.#err.write(`${text}\n`);
    }
}
