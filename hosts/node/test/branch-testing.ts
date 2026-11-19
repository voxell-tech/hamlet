/** What the tests of the agent's branch share. */
import assert from "node:assert/strict";
import type { FilesAnswer, FilesPart, Save } from "../src/agent/messages.ts";
import { from, named, type Sent, type Woken } from "./agent-testing.ts";

export const A = "ab".repeat(20);
export const B = "cd".repeat(20);

export function bytes(text: string): string {
    return Buffer.from(text).toString("base64");
}

export function part(
    given: Partial<FilesPart> & Pick<FilesPart, "tip">,
): FilesPart {
    return { type: "part", files: [], removed: [], ...given };
}

/** A profile that answers each `Files` with the next of `answers`. */
export function profileAnswering(...answers: FilesAnswer[]) {
    return {
        files: () => {
            const next = answers.shift();
            assert.ok(next, "the profile was asked for more files");
            return next;
        },
        save: (): { type: "saved"; tip: string } => ({ type: "saved", tip: B }),
    };
}

export async function filled(agent: Woken): Promise<string | null | undefined> {
    return (
        await agent.c.db.get<{ filled: string | null }>(
            "SELECT filled FROM agent",
        )
    )?.filled;
}

export async function say(agent: Woken, text: string): Promise<void> {
    await agent.actions.submit(agent.by(from.realm), {
        id: `line-${text}`,
        input: { type: "line", text },
    });
    await agent.idle();
}

export async function readPrompt(agent: Woken): Promise<string> {
    return (await agent.actions.read_prompt(agent.by(from.client))).text;
}

export function saves(sent: readonly Sent[]): Save[] {
    return named(sent, "save").map((call) => call.message as Save);
}
