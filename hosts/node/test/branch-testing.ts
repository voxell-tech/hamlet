/** Helpers shared by the tests of the agent's branch. */
import assert from "node:assert/strict";
import type {
    FilesAnswer,
    FilesPart,
    Piece,
    Save,
} from "../src/agent/messages/profile.ts";
import { from, named, type Sent, type Woken } from "./agent-testing.ts";

export const A = "ab".repeat(20);
export const B = "cd".repeat(20);

export function bytes(text: string): string {
    return Buffer.from(text).toString("base64");
}

/** All of `text` as the one piece of the file at `path`. */
export function piece(path: string, text: string): Piece {
    return { path, offset: 0, bytes: bytes(text), last: true };
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

export function saves(sent: readonly Sent[]): Save[] {
    return named(sent, "save").map((call) => call.message as Save);
}
