import assert from "node:assert/strict";
import { test } from "node:test";
import { eventually, inFolder } from "../native/support.ts";
import { answer, type Demo, startDemo } from "./support.ts";

/** The demo opens no sandbox on Windows, so its workspace cannot start. */
const SKIP = { skip: process.platform === "win32" };

/**
 * A faux turn, a restarted Node host's included, ends within 3 s here; this
 * is twenty times that.
 */
const TURN_MS = 60_000;

const NOISE = /\b(WARN|ERROR)\b/;

/** Waits until agent `n`'s current chat holds an item `found` accepts. */
async function chatHolds(
    demo: Demo,
    n: number,
    found: (item: { kind: string; text?: string }) => boolean,
): Promise<void> {
    await eventually(
        async () => {
            const line = await demo.ask(`pi chat ${n}`);
            if (!line.startsWith(`chat/${n} `)) return [];
            const chat = answer(line, `chat/${n}`) as {
                items: { kind: string; text?: string }[];
            };
            return chat.items;
        },
        (items) => items.some(found),
        TURN_MS,
    );
}

/**
 * Asserts that agent 3 holds agent 2's tell once: a later letter from agent 2
 * reaches it after any copy of the first, as one agent's letters arrive in
 * the order they were sent.
 */
async function heldOnce(demo: Demo): Promise<void> {
    assert.equal(await demo.ask("agent 2 tell t2 hire#3 later"), "sent/t2 1");
    assert.equal(await demo.ask("agent 3 await letter 2"), "took/3/letter 2");
}

/** Founds `h1`, whose mayor is agent 2, and hires agents 3 to `last` under it, each live. */
async function hamlet(demo: Demo, last: number): Promise<void> {
    assert.equal(await demo.ask("realm found h1"), "founded/h1 2");
    assert.equal(await demo.ask("realm live 2"), "live/2 1");
    for (let n = 3; n <= last; n++) {
        assert.equal(
            await demo.ask(`realm hire 2 c${n} helper`),
            `hired/c${n} ${n}`,
        );
        assert.equal(await demo.ask(`realm live ${n}`), `live/${n} 1`);
    }
}

test(
    "a chief's line said through the realm gets the real agent's reply",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir, ["--realm-actor"]);
            assert.match(
                await demo.ask("realm say 1 l1 hello there"),
                /^taken\/l1 \d+$/,
            );
            await chatHolds(
                demo,
                1,
                (item) =>
                    item.kind === "assistant" &&
                    item.text === "Heard: hello there",
            );
            assert.equal(await demo.stop(), 0);
        });
    },
);

test(
    "a hire's tell made by its model reaches the other hire once",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir, ["--realm-actor"]);
            await hamlet(demo, 3);
            assert.match(
                await demo.ask("realm say 2 s1 tell 3: hello"),
                /^taken\/s1 /,
            );
            await chatHolds(demo, 2, (item) => item.text === "Done: Sent.");
            await heldOnce(demo);
            assert.equal(await demo.stop(), 0);
        });
    },
);

test(
    "a letter told along a laid road reaches the other hire",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir, ["--realm-actor"]);
            await hamlet(demo, 4);
            assert.equal(
                await demo.ask("realm road hire#3 hire#4"),
                "road/hire#3/hire#4 1",
            );
            assert.equal(
                await demo.ask("agent 3 tell t1 hire#4 by road"),
                "sent/t1 1",
            );
            assert.equal(
                await demo.ask("agent 4 await letter 1"),
                "took/4/letter 1",
            );
            assert.equal(await demo.stop(), 0);
        });
    },
);

test(
    "a demo with the realm actor serves the counter kinds it names beside it",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir, ["--kind", "x", "--realm-actor"]);
            assert.equal(await demo.ask("realm found h1"), "founded/h1 2");
            assert.equal(await demo.ask("get k1"), "x/k1 0");
            assert.equal(await demo.stop(), 0);
        });
    },
);

test(
    "two client calls under one request id are one request",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir, ["--realm-actor"]);
            const first = await demo.ask("pi new 1 --id n1");
            assert.match(first, /^new\/1 \{"number":\d+\}$/);
            assert.equal(await demo.ask("pi new 1 --id n1"), first);
            assert.notEqual(await demo.ask("pi new 1 --id n2"), first);
            assert.equal(await demo.stop(), 0);
        });
    },
);

test(
    "a Node host killed after the realm answered a tell resumes the turn, and the letter is held once",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir, ["--realm-actor"]);
            await hamlet(demo, 3);
            const before = await demo.ask("pi pid");
            assert.equal(await demo.ask("pi die-after tell"), "die/tell 1");
            assert.match(
                await demo.ask("realm say 2 s1 tell 3: once"),
                /^taken\/s1 /,
            );
            await chatHolds(demo, 2, (item) => item.text === "Done: Sent.");
            assert.notEqual(await demo.ask("pi pid"), before);
            await heldOnce(demo);
            assert.equal(await demo.stop(), 0);
        });
    },
);

test(
    "a demo with the realm actor starts, stops and starts again with its agents and logs no warning",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const first = startDemo(dir, ["--realm-actor"]);
            await hamlet(first, 6);
            assert.equal(await first.stop(), 0);
            assert.doesNotMatch(first.stderr(), NOISE);
            const second = startDemo(dir, ["--realm-actor"]);
            assert.equal(await second.ask("realm live 6"), "live/6 1");
            assert.equal(await second.stop(), 0);
            assert.doesNotMatch(second.stderr(), NOISE);
        });
    },
);
