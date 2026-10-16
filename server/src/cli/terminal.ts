import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import type { Connection } from "../connection.ts";
import {
    type ClientMessage,
    checkEdit,
    type Edit,
    type ServerMessage,
    type State,
} from "../protocol.ts";
import { HOME, MAYOR } from "../record.ts";
import { connectChief, type ServerAddress } from "./client.ts";
import type { CommandIO } from "./io.ts";

/** The chief documents accepted by the terminal's edit command. */
export const DOCUMENTS = [
    "charter",
    "character",
    "notes",
    "limits",
    "settings",
] as const;
/** A chief document editable through the terminal. */
export type DocumentName = (typeof DOCUMENTS)[number];
/** A terminal operation performed over an authenticated chief connection. */
export type TerminalCommand =
    | { name: "edit"; doc: DocumentName; hamlet?: string }
    | { name: "login"; provider: string; key: boolean }
    | { name: "models"; action: "list" | "refresh" | "try"; model?: string };

/** The chief's document editor, sign-in and model catalogue over the protocol. */
export async function terminal(
    command: TerminalCommand,
    address: ServerAddress,
    env: Readonly<NodeJS.ProcessEnv>,
    io: CommandIO,
): Promise<void> {
    const { connection, state } = await connectChief(address);
    const request = async (
        message: ClientMessage & { id: string },
    ): Promise<ServerMessage> => {
        connection.send(message);
        for (;;) {
            const answer = await connection.next();
            if (
                answer.type === "fault" ||
                ("to" in answer && answer.to === message.id)
            )
                return answer;
        }
    };
    try {
        if (command.name === "login") {
            await signIn(connection, command.provider, command.key, io);
            return;
        }
        if (command.name === "models") {
            const answer = await request(
                command.action === "try"
                    ? {
                          type: "check",
                          id: randomUUID(),
                          what: "model",
                          ...(command.model === undefined
                              ? {}
                              : { model: command.model }),
                      }
                    : {
                          type: "read",
                          id: randomUUID(),
                          what: "models",
                          refresh: command.action === "refresh",
                      },
            );
            if (
                answer.type === "fault" ||
                (answer.type === "reply" && answer.kind === "error")
            )
                throw new Error(answer.text);
            if (answer.type === "reply") io.stdout.write(`${answer.text}\n`);
            else if (answer.type === "read" && "models" in answer)
                for (const model of answer.models)
                    io.stdout.write(
                        `${model.name}\t${model.price === null ? "unpriced" : `$${model.price.input}/M in, $${model.price.output}/M out`}\t${model.context ?? "unknown"} context\t${model.allowed ? "allowed" : "not allowed"}${model.note ? `\t${model.note}` : ""}\n`,
                    );
            else throw new Error("The server did not send the models.");
            return;
        }
        const message = await editDocument(
            command.doc,
            state,
            request,
            command.hamlet ?? HOME,
            env,
            io,
        );
        const answer = await request(message);
        if (answer.type !== "reply")
            throw new Error(
                answer.type === "fault"
                    ? answer.text
                    : "The server did not answer the edit.",
            );
        if (answer.kind === "error") throw new Error(answer.text);
        io.stderr.write(`  (${answer.text})\n`);
    } finally {
        await connection.close();
    }
}

async function editDocument(
    doc: DocumentName,
    state: State,
    request: (
        message: ClientMessage & { id: string },
    ) => Promise<ServerMessage>,
    hamlet: string,
    env: Readonly<NodeJS.ProcessEnv>,
    io: CommandIO,
): Promise<Edit> {
    const agent = { hamlet, agent: MAYOR };
    let text: string;
    if (doc === "limits") {
        const hamlet = state.hamlets.find(
            (hamlet) => hamlet.id === agent.hamlet,
        );
        if (hamlet === undefined)
            throw new Error(`no hamlet named \`${agent.hamlet}\``);
        text = `${JSON.stringify(hamlet.limits, null, 2)}\n`;
    } else if (doc === "settings") {
        const { model, reflectEvery, allowNet, endpoints } = state.settings;
        text = `${JSON.stringify({ model, reflectEvery, allowNet, endpoints: endpoints.map(({ name, baseUrl }) => ({ name, baseUrl })) }, null, 2)}\n`;
    } else {
        const read = await request(
            doc === "charter"
                ? { type: "read", id: randomUUID(), what: "charter" }
                : {
                      type: "read",
                      id: randomUUID(),
                      what: "papers",
                      agent,
                      full: true,
                  },
        );
        if (read.type === "fault" || read.type === "reply")
            throw new Error(read.text);
        if (read.type !== "read")
            throw new Error("The server did not send the document.");
        if (doc === "charter" && "charter" in read) text = read.charter.text;
        else if (doc !== "charter" && "papers" in read) text = read.papers[doc];
        else throw new Error("The server sent another document.");
    }
    const changed = await inEditor(doc, text, env, io);
    const reply = { type: "edit", id: randomUUID() } as const;
    if (doc === "limits" || doc === "settings") {
        const value: unknown = JSON.parse(changed);
        const parsed = checkEdit(
            doc === "limits"
                ? { ...reply, doc, hamlet: agent.hamlet, whole: value }
                : { ...reply, doc, set: value },
        );
        if (!parsed.ok) throw new Error(parsed.error.text);
        return parsed.value;
    }
    if (doc === "charter") return { ...reply, doc, text: changed };
    return { ...reply, doc, agent, text: changed };
}

async function inEditor(
    doc: DocumentName,
    text: string,
    env: Readonly<NodeJS.ProcessEnv>,
    io: CommandIO,
): Promise<string> {
    const editor = env.EDITOR;
    if (!editor) throw new Error("Set EDITOR to the editor to open.");
    const dir = await mkdtemp(join(tmpdir(), "hamlet-edit-"));
    const path = join(
        dir,
        `${doc}.${doc === "limits" || doc === "settings" ? "json" : "md"}`,
    );
    try {
        await writeFile(path, text, { mode: 0o600 });
        const child = spawn(
            "sh",
            ["-c", `exec ${editor} "$1"`, "hamlet-edit", path],
            { env, stdio: "pipe" },
        );
        io.stdin.pipe(child.stdin);
        child.stdout.pipe(io.stdout, { end: false });
        child.stderr.pipe(io.stderr, { end: false });
        const exit = await new Promise<number | string>((resolve, reject) => {
            child.once("error", reject);
            child.once("exit", (code, signal) => resolve(code ?? signal ?? ""));
        });
        io.stdin.unpipe(child.stdin);
        if (exit !== 0)
            throw new Error(
                `The editor ended with ${exit}. Nothing was saved.`,
            );
        return await readFile(path, "utf8");
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
}

async function signIn(
    connection: Connection,
    provider: string,
    key: boolean,
    io: CommandIO,
): Promise<void> {
    // Readline edits a TTY's line in raw mode. With no output it echoes nothing.
    const lines = createInterface({
        input: io.stdin,
        terminal: io.stdin.isTTY,
    });
    const input = lines[Symbol.asyncIterator]();
    lines.on("SIGINT", () => process.kill(process.pid, "SIGINT"));
    try {
        connection.send({
            type: "login",
            id: randomUUID(),
            start: { provider, method: key ? "key" : "subscription" },
        });
        for (;;) {
            const message = await connection.next();
            if (
                message.type === "fault" ||
                (message.type === "reply" && message.kind === "error")
            )
                throw new Error(message.text);
            if (message.type !== "login") continue;
            const step = message.step;
            if ("url" in step)
                io.stderr.write(
                    `${step.url}\n${step.instructions ? `${step.instructions}\n` : ""}`,
                );
            if ("code" in step) io.stderr.write(`Code: ${step.code}\n`);
            if ("progress" in step) io.stderr.write(`${step.progress}\n`);
            if ("failed" in step) throw new Error(step.failed);
            if ("done" in step) {
                io.stderr.write(`Signed in to ${step.done}.\n`);
                return;
            }
            if ("prompt" in step) {
                io.stderr.write(`${step.prompt}\n`);
                const line = await input.next();
                if (line.done)
                    throw new Error("Sign-in needs an answer; input ended.");
                connection.send({
                    type: "login",
                    id: randomUUID(),
                    reply: { login: message.login, value: line.value },
                });
            }
        }
    } finally {
        lines.close();
        io.stdin.pause();
    }
}
