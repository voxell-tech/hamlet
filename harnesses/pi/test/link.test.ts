import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { afterEach, expect, test } from "vitest";
import { isRuntimeMessage, linkSchema } from "../src/link.ts";

const MAIN = join(import.meta.dirname, "..", "src", "main.ts");
const folders: string[] = [];

afterEach(() => {
    for (const folder of folders.splice(0))
        rmSync(folder, { recursive: true, force: true });
});

test("the checked-in schema is the link's", () => {
    const kept: unknown = JSON.parse(
        readFileSync(
            join(import.meta.dirname, "..", "link.schema.json"),
            "utf8",
        ),
    );
    expect(kept).toEqual(linkSchema());
});

/** A field of a JSON object read off the link, or undefined. */
function field(value: unknown, name: string): unknown {
    return typeof value === "object" && value !== null && name in value
        ? Object.entries(value).find(([key]) => key === name)?.[1]
        : undefined;
}

test("a runtime speaks only the link's messages from hello to an answer, and exits when the server goes", async () => {
    const folder = mkdtempSync(join(tmpdir(), "hamlet-pi-link-"));
    folders.push(folder);
    const faux = join(folder, "faux.json");
    writeFileSync(faux, JSON.stringify({ agents: { "home:0": ["hi"] } }));
    const child = spawn(
        process.execPath,
        [
            "--disable-warning=ExperimentalWarning",
            MAIN,
            ...["--runtime", "pi-test", "--store", join(folder, "pi.sqlite")],
            ...["--link", "stdio", "--fresh", "--faux", faux],
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
            `${JSON.stringify({ v: 1, id: `s${sent}`, ...message })}\n`,
        );
    };
    const heard: unknown[] = [];
    for await (const line of createInterface({ input: child.stdout })) {
        const message: unknown = JSON.parse(line);
        heard.push(message);
        const re = field(message, "id");
        switch (field(message, "type")) {
            case "hello":
                send({ type: "welcome", re, version: 1, tools: [] });
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
                send({ type: "allowance", re });
                break;
            case "usage":
                send({ type: "charged", re });
                break;
            case "prompt":
                send({ type: "sections", re, sections: [] });
                break;
        }
        const item = field(message, "item");
        if (field(item, "kind") === "assistant" && field(item, "text") === "hi")
            break;
    }
    child.stdin.end();
    expect(await exited).toBe(0);
    expect(heard.filter((message) => !isRuntimeMessage(message))).toEqual([]);
    expect(heard.map((message) => field(message, "type"))).toContain(
        "admitted",
    );
});
