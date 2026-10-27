import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { afterEach, expect, test } from "vitest";
import { isRuntimeMessage, PROTOCOL_VERSION } from "../src/protocol.ts";

const MAIN = join(import.meta.dirname, "..", "src", "main.ts");
const folders: string[] = [];

afterEach(() => {
    for (const folder of folders.splice(0))
        rmSync(folder, { recursive: true, force: true });
});

/** A field of a JSON object read off the protocol, or undefined. */
function field(value: unknown, name: string): unknown {
    return typeof value === "object" && value !== null && name in value
        ? Object.entries(value).find(([key]) => key === name)?.[1]
        : undefined;
}

/** Runs a fresh runtime on `script`, answering for the server with `tools` and the results of `answer`, until `done` holds for an item; returns every message heard and the exit code. */
async function converse(
    script: unknown,
    tools: unknown[],
    answer: (operation: unknown) => string,
    done: (item: unknown) => boolean,
) {
    const folder = mkdtempSync(join(tmpdir(), "hamlet-pi-protocol-"));
    folders.push(folder);
    const faux = join(folder, "faux.json");
    writeFileSync(faux, JSON.stringify(script));
    const child = spawn(
        process.execPath,
        [
            "--disable-warning=ExperimentalWarning",
            MAIN,
            ...["--runtime", "pi-test", "--store", join(folder, "pi.sqlite")],
            ...["--transport", "stdio", "--fresh", "--faux", faux],
        ],
        { stdio: ["pipe", "pipe", "inherit"] },
    );
    const exited = new Promise<number | null>((resolve) =>
        child.once("exit", resolve),
    );
    let sent = 0;
    const send = (message: Record<string, unknown>) => {
        sent += 1;
        child.stdin.write(
            `${JSON.stringify({ v: PROTOCOL_VERSION, id: `s${sent}`, ...message })}\n`,
        );
    };
    const heard: unknown[] = [];
    for await (const line of createInterface({ input: child.stdout })) {
        const message: unknown = JSON.parse(line);
        heard.push(message);
        const re = field(message, "id");
        switch (field(message, "type")) {
            case "hello":
                send({ type: "welcome", re, tools, windows: {} });
                send({
                    type: "create",
                    key: "b1",
                    agent: "home:0",
                    errand: false,
                    model: { provider: "faux", id: "faux-1" },
                });
                break;
            case "created":
                send({
                    type: "submit",
                    conversation: field(message, "conversation"),
                    request: "r1",
                    content: "Hello.",
                    steer: false,
                });
                break;
            case "attempt":
                send({
                    type: "allowance",
                    re,
                    selection: {
                        model: { provider: "faux", id: "faux-1" },
                        price: {
                            input: 0,
                            output: 0,
                            cacheRead: 0,
                            cacheWrite: 0,
                        },
                    },
                });
                break;
            case "usage":
                send({ type: "charged", re });
                break;
            case "scrub":
                send({ type: "scrubbed", re, value: field(message, "value") });
                break;
            case "prompt":
                send({ type: "sections", re, sections: [] });
                break;
            case "yield":
                send({ type: "verdict", re });
                break;
            case "allow":
                send({ type: "allowed", re });
                break;
            case "operation":
                send({
                    type: "result",
                    re,
                    text: answer(message),
                    error: false,
                });
                break;
        }
        if (done(field(message, "item"))) break;
    }
    child.stdin.end();
    return { heard, exited: await exited };
}

/** Whether `item` is the assistant's answer `text`. */
const answered = (text: string) => (item: unknown) =>
    field(item, "kind") === "assistant" && field(item, "text") === text;

test("a runtime speaks only the protocol's messages from hello to an answer, and exits when the server goes", async () => {
    const { heard, exited } = await converse(
        { agents: { "home:0": ["hi"] } },
        [],
        () => "",
        answered("hi"),
    );
    expect(exited).toBe(0);
    expect(heard.filter((message) => !isRuntimeMessage(message))).toEqual([]);
    expect(heard.map((message) => field(message, "type"))).toContain(
        "admitted",
    );
});

test("a tool that retains the tail keeps the end of a long result, and one without keeps the start", async () => {
    const lines = Array.from({ length: 3000 }, (_, n) => `line ${n + 1}`);
    const tool = (name: string) => ({
        name,
        description: `Says ${name}.`,
        parameters: { type: "object", properties: {} },
        replay: "safe",
    });
    const calls = ["tail", "head"].map((name) => ({ name, arguments: {} }));
    const { heard } = await converse(
        { agents: { "home:0": [{ calls }, "done"] } },
        [{ ...tool("tail"), retain: "tail" }, tool("head")],
        () => lines.join("\n"),
        answered("done"),
    );
    const kept = new Map(
        heard
            .map((message) => field(message, "item"))
            .filter((item) => field(item, "kind") === "tool_result")
            .map((item) => [field(item, "tool"), String(field(item, "text"))]),
    );
    expect(kept.get("tail")).toContain("line 3000");
    expect(kept.get("tail")).not.toContain("line 1\n");
    expect(kept.get("head")).toContain("line 1\n");
    expect(kept.get("head")).not.toContain("line 3000");
});
