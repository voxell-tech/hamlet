import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import {
    ActorError,
    Address,
    actor,
    type CallOptions,
    type Calls,
} from "../../src/actor/actor/mod.ts";
import type { JsonCompatValue } from "../../src/actor/common/encoding.ts";
import { agentActor } from "../../src/agent/actor.ts";
import { cost, MOST_PART } from "../../src/agent/branch.ts";
import type {
    AgentActions,
    AgentInput,
    InboxCursor,
    InboxPart,
} from "../../src/agent/messages/agent.ts";
import { fakeFiles } from "../agent-testing.ts";
import { fauxModels } from "../testing.ts";
import { inFolder, startLocalHost } from "./support.ts";

const CALL: CallOptions = { deadlineMs: 10_000 };

const ABSORBED = 4;
const KEPT = 3;

async function send(
    c: Calls,
    n: number,
    name: keyof AgentActions,
    message: unknown,
): Promise<unknown> {
    const to = Address.of<Record<string, (message: unknown) => unknown>>(
        "agent",
        String(n),
    );
    try {
        return { answer: await c.call(to, name, [message], CALL) };
    } catch (error) {
        if (!(error instanceof ActorError)) throw error;
        return { error: `${error.group}.${error.code}: ${error.message}` };
    }
}

const realm = actor({ actions: { send } });

const HIRE: AgentInput = {
    kind: "hand",
    task: "standing-1",
    place: {
        version: 1,
        hires: [],
        roads: [],
        tools_off: [],
        manual: false,
        agrees_roads: false,
    },
    copies: {},
};

/** `size` letters, the same for the same `seed`. */
function letters(size: number, seed: number): string {
    const bytes = Buffer.alloc(size);
    let state = seed;
    for (let i = 0; i < size; i += 1) {
        state = (state * 1_103_515_245 + 12_345) >>> 0;
        bytes[i] = 97 + ((state >>> 16) % 26);
    }
    return bytes.toString("latin1");
}

/** Each file's length and sha256, by path. */
function digests(files: Record<string, string>, prefix: string) {
    return Object.fromEntries(
        Object.entries(files)
            .filter(([path]) => path.startsWith(prefix))
            .map(([path, text]) => [
                path.slice(prefix.length),
                `${text.length} ${createHash("sha256").update(text).digest("hex")}`,
            ]),
    );
}

/**
 * Starts the absorbed and the kept hire's agents on the native host, drives
 * the realm's inbox step over them, and gives each file's digest in the kept
 * hire's `inbox/from-hire-4/`, and what the absorbed hire's `inbox/` holds.
 */
async function moved(inbox: Record<string, string>) {
    const disks: Record<number, Record<string, string>> = {
        [ABSORBED]: { ...inbox },
        [KEPT]: {},
    };
    const { faux, models, model } = fauxModels();
    faux.setResponses([fauxAssistantMessage("Done.")]);
    const agent = agentActor({
        models,
        model,
        files: (n) => fakeFiles(disks[n] ?? {}),
    });
    return inFolder(async (dir) => {
        const host = await startLocalHost(dir, { agent, realm });
        const r = host.at("realm", "local");
        await host.create(r);
        for (const n of [ABSORBED, KEPT]) {
            await host.create(
                host.at("agent", String(n)),
                HIRE as unknown as JsonCompatValue,
            );
        }
        const call = async (
            n: number,
            name: keyof AgentActions,
            message: unknown,
        ) => {
            const got = (await host.call(
                r,
                "send",
                [n, name, message],
                CALL,
            )) as {
                answer?: unknown;
                error?: string;
            };
            assert.ok(!("error" in got), `${name} of agent ${n}: ${got.error}`);
            return got.answer;
        };
        let after: InboxCursor | undefined;
        try {
            do {
                const part = (await call(
                    ABSORBED,
                    "read_inbox",
                    after === undefined ? {} : { after },
                )) as InboxPart;
                await call(KEPT, "take_inbox", {
                    from: ABSORBED,
                    files: part.files,
                });
                const paths = part.files
                    .filter((piece) => piece.last)
                    .map((piece) => piece.path);
                if (paths.length > 0)
                    await call(ABSORBED, "drop_inbox", { paths });
                after = part.next;
            } while (after !== undefined);
        } finally {
            await host.shutdown();
        }
        return {
            kept: digests(disks[KEPT] ?? {}, "inbox/from-hire-4/"),
            left: Object.keys(disks[ABSORBED] ?? {}),
        };
    });
}

test("a file of several parts, an empty one and ones at a part's edge reach the kept hire byte for byte on the native host", async () => {
    const piece = MOST_PART - cost("edge/piece", 0);
    const inbox = {
        "inbox/scan.pdf": letters(3_000_000, 1),
        "inbox/empty.txt": "",
        "inbox/edge/part": letters(MOST_PART, 2),
        "inbox/edge/part-and-one": letters(MOST_PART + 1, 3),
        "inbox/edge/piece": letters(piece, 4),
        "inbox/edge/piece-and-one": letters(piece + 1, 5),
        "memory/notes.md": "# Notes\n",
    };

    const { kept, left } = await moved(inbox);

    assert.deepEqual(kept, digests(inbox, "inbox/"));
    assert.deepEqual(left, ["memory/notes.md"]);
});

test("ten 2 MB files beside 900 small ones reach the kept hire byte for byte on the native host", async () => {
    const inbox: Record<string, string> = { "memory/notes.md": "# Notes\n" };
    for (let i = 1; i <= 900; i += 1) {
        inbox[`inbox/bulk/f${String(i).padStart(3, "0")}.txt`] =
            `bulk file ${i}\n`;
    }
    for (let i = 1; i <= 10; i += 1) {
        inbox[`inbox/bulk/big${i}.bin`] = letters(2_000_000, 10 + i);
    }

    const { kept, left } = await moved(inbox);

    assert.deepEqual(kept, digests(inbox, "inbox/"));
    assert.deepEqual(left, ["memory/notes.md"]);
});
