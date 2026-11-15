import assert from "node:assert/strict";
import { test } from "node:test";
import { inFolder } from "../native/support.ts";
import { answer, startDemo } from "./support.ts";

const SKIP = { skip: process.platform === "win32" };

test(
    "a subscribe the TypeScript agent refuses to a client reaches the Rust client as auth.forbidden, while ticked stays open to it",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir);
            await demo.ask("make 1");
            const refused = answer(await demo.ask("watch 1 noted"), "watch/1");
            assert.equal(await demo.ask("watch 1 ticked"), "watch/1 1");
            await demo.ask("tell 1 tick 50");
            await demo.streamedLine(/^watch\/1\/ticked \[2\]$/);
            assert.equal(await demo.stop(), 0, demo.stderr());
            assert.deepEqual(refused, {
                error: {
                    group: "auth",
                    code: "forbidden",
                    message: "Forbidden",
                    metadata: null,
                },
            });
        });
    },
);

test(
    "a subscribe to an event the agent kind does not declare is let through and gives nothing, while a declared event's broadcasts arrive",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir);
            await demo.ask("make 1");
            assert.equal(await demo.ask("watch 1 undeclared"), "watch/1 1");
            assert.equal(await demo.ask("watch 1 ticked"), "watch/1 1");
            await demo.ask("tell 1 tick 50");
            await demo.streamedLine(/^watch\/1\/ticked \[5\]$/);
            const streamed = demo
                .streamed()
                .filter((line) => line.startsWith("watch/1/"));
            assert.equal(await demo.stop(), 0, demo.stderr());
            assert.deepEqual(
                streamed.filter((line) => !line.startsWith("watch/1/ticked ")),
                [],
            );
        });
    },
);
