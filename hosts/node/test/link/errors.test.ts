import assert from "node:assert/strict";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { inFolder } from "../native/support.ts";
import {
    answer,
    childrenOf,
    type Demo,
    hasEnded,
    startDemo,
} from "./support.ts";

const SKIP = { skip: process.platform !== "linux" };

/** How long a Node host may outlive its demo host. */
const GONE_WAIT_MS = 2_000;

/** The number a line `<name> <number>` holds. */
function number(line: string, name: string): number {
    const value = answer(line, name);
    assert.equal(typeof value, "number", line);
    return value as number;
}

async function nodePid(demo: Demo, n: number): Promise<number> {
    return number(await demo.ask(`tell ${n} pid`), `agent/${n}/pid`);
}

/** Waits until the Node host `node` has ended, at most `GONE_WAIT_MS`. */
async function ended(node: number): Promise<void> {
    const until = Date.now() + GONE_WAIT_MS;
    while (!hasEnded(node)) {
        if (Date.now() > until) {
            process.kill(node, "SIGKILL");
            assert.fail(`the Node host ${node} did not end`);
        }
        await sleep(20);
    }
}

/** Stops `demo`, and checks that it and the Node host `node` ended. */
async function stopped(demo: Demo, node: number): Promise<void> {
    assert.equal(await demo.stop(), 0, demo.stderr());
    await ended(node);
}

test(
    "a Rust actor's call is answered by a TypeScript actor that names its caller, in the demo's one Node child",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir);
            assert.equal(await demo.ask("make 1"), "agent/1 1");
            assert.equal(await demo.ask("make 1"), "agent/1 0");
            assert.equal(await demo.ask("tell 1 note"), "agent/1/note 1");
            assert.equal(
                await demo.ask("tell 1 note --count 3"),
                "agent/1/note 4",
            );
            await demo.logged(/^agent\/1 called by realm\/local$/m);
            const node = await nodePid(demo, 1);
            assert.deepEqual(childrenOf(demo.pid), [node]);
            await stopped(demo, node);
        });
    },
);

test(
    "a TypeScript agent's call to the realm reads message.wrong_caller with its metadata, across both hops",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir);
            await demo.ask("make 1");
            const read = answer(
                await demo.ask("tell 1 ask realm local secrets"),
                "agent/1/ask",
            );
            await stopped(demo, await nodePid(demo, 1));
            assert.deepEqual(read, {
                error: {
                    group: "message",
                    code: "wrong_caller",
                    message: "agent/1 may not send 'secrets'.",
                    metadata: {
                        action: "secrets",
                        caller: { actor: { kind: "agent", key: "1" } },
                    },
                },
            });
        });
    },
);

test(
    "an agent's call of the realm's tell reads message.wrong_caller, so the realm calls no agent for it",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir);
            await demo.ask("make 1");
            await demo.ask("make 2");
            const read = answer(
                await demo.ask(
                    'tell 1 ask realm local tell {"to":{"kind":"agent","key":"2"},"name":"note","args":""}',
                ),
                "agent/1/ask",
            );
            const notes = await demo.ask("tell 2 count");
            await stopped(demo, await nodePid(demo, 1));
            assert.deepEqual(read, {
                error: {
                    group: "message",
                    code: "wrong_caller",
                    message: "agent/1 may not send 'tell'.",
                    metadata: {
                        action: "tell",
                        caller: { actor: { kind: "agent", key: "1" } },
                    },
                },
            });
            assert.equal(notes, "agent/2/count 0");
        });
    },
);

test(
    "an agent's call to an agent, a pair the demo's route table does not list, is refused: the agent reads actor.refused with its metadata, and the log holds the refusal",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir);
            await demo.ask("make 1");
            await demo.ask("make 2");
            const read = answer(
                await demo.ask("tell 1 ask agent 2 note"),
                "agent/1/ask",
            );
            await demo.logged(
                /refused an operation its route table does not allow/,
            );
            assert.equal(await demo.ask("tell 2 count"), "agent/2/count 0");
            await stopped(demo, await nodePid(demo, 1));
            assert.deepEqual(read, {
                error: {
                    group: "actor",
                    code: "refused",
                    message:
                        "Actor operation 'call' is refused: agent may not reach the kind 'agent'.",
                    metadata: {
                        operation: "call",
                        from: { actor: "agent" },
                        to: "agent",
                    },
                },
            });
        });
    },
);

test(
    "in one flow the realm calls an agent that calls the realm: each reads the other as its caller, and the realm's u64 from 2^32 is a number in the agent",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir);
            await demo.ask("make 1");
            assert.equal(await demo.ask("tell 1 note"), "agent/1/note 1");
            const who = answer(
                await demo.ask("tell 1 ask realm local who"),
                "agent/1/ask",
            );
            const powers = [];
            for (const bits of [31, 32, 52]) {
                powers.push(
                    answer(
                        await demo.ask(`tell 1 ask realm local power ${bits}`),
                        "agent/1/ask",
                    ),
                );
            }
            await demo.logged(/^agent\/1 called by realm\/local$/m);
            await demo.logged(/the realm was asked who calls.*agent.*"1"/);
            await stopped(demo, await nodePid(demo, 1));
            assert.deepEqual(who, {
                answer: { actor: { kind: "agent", key: "1" } },
                type: "object",
            });
            assert.deepEqual(powers, [
                { answer: 2 ** 31, type: "number" },
                { answer: 2 ** 32, type: "number" },
                { answer: 2 ** 52, type: "number" },
            ]);
        });
    },
);

test(
    "a TypeScript action's coded error reaches the realm with its group, code, message and metadata",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir);
            await demo.ask("make 1");
            const read = answer(
                await demo.ask("tell 1 refuse"),
                "agent/1/refuse",
            );
            await stopped(demo, await nodePid(demo, 1));
            assert.deepEqual(read, {
                error: {
                    group: "user",
                    code: "demo_refused",
                    message: "refused by the demo",
                    metadata: { n: 2 ** 32, list: [1, "two"] },
                },
            });
        });
    },
);

test(
    "a count on a chain of one-shots keeps growing in the next Node host once the first was killed",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir);
            await demo.ask("make 1");
            await demo.ask("make 2");
            const first = await nodePid(demo, 1);
            assert.equal(await demo.ask("tell 1 tick 50"), "agent/1/tick 0");
            const ticks = async () =>
                number(await demo.ask("tell 1 ticks"), "agent/1/ticks");
            const grown = async (above: number) => {
                const until = Date.now() + 10_000;
                let now = await ticks();
                while (now <= above) {
                    assert.ok(Date.now() < until, `ticks stayed at ${now}`);
                    await sleep(50);
                    now = await ticks();
                }
                return now;
            };
            const before = await grown(2);
            assert.match(
                await demo.ask("tell 2 begin"),
                /^error: link\.lost: /,
            );
            await ended(first);
            assert.equal(await demo.ask("link"), "link/ready 1");
            await grown(before);
            const second = await nodePid(demo, 1);
            assert.notEqual(second, first);
            await stopped(demo, second);
        });
    },
);
