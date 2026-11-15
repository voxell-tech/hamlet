import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import {
    type ActionsOf,
    ActorError,
    Address,
    actor,
    type CallOptions,
    isActorErrorCode,
    UserError,
} from "../../src/actor/actor/mod.ts";
import { db } from "../../src/actor/db/mod.ts";
import { inFolder, startLocalHost } from "./support.ts";

const CALL: CallOptions = { deadlineMs: 10_000 };

const thrower = actor({
    actions: {
        wrongCaller: () => {
            throw new ActorError(
                "message",
                "wrong_caller",
                "This caller may not send 'wrongCaller'.",
                {
                    metadata: {
                        action: "wrongCaller",
                        n: 2 ** 40,
                        list: [1, "x"],
                    },
                },
            );
        },
        noRoom: () => {
            throw new UserError("no room", {
                code: "no_room",
                metadata: { n: 1 },
            });
        },
        diskFull: () => {
            throw new Error("the disk is full");
        },
        slow: async () => {
            await sleep(2_000);
            return 1;
        },
    },
});

/** What a caller reads of an error. */
function read(error: unknown) {
    assert.ok(error instanceof ActorError, String(error));
    return {
        group: error.group,
        code: error.code,
        message: error.message,
        // The bridge writes no metadata as null.
        metadata: error.metadata ?? null,
    };
}

/** Counts the calls that reached it. */
const tally = actor({
    db: db({
        onMigrate: async (raw) => {
            await raw.execute("CREATE TABLE IF NOT EXISTS seen (n INTEGER)");
        },
    }),
    actions: {
        bump: async (c) => {
            await c.db.run("INSERT INTO seen (n) VALUES (1)");
        },
        count: async (c) =>
            (await c.db.get<{ n: number }>("SELECT count(*) AS n FROM seen"))
                ?.n ?? 0,
    },
});

const relay = actor({
    actions: {
        /** Calls `tally/t`'s `bump`, and answers what it read of an error. */
        bump: async (c, deadlineMs: number) => {
            try {
                await c.call(
                    Address.of<ActionsOf<typeof tally>>("tally", "t"),
                    "bump",
                    [],
                    { deadlineMs },
                );
                return null;
            } catch (error) {
                assert.ok(error instanceof ActorError, String(error));
                return {
                    code: `${error.group}.${error.code}`,
                    metadata: error.metadata,
                };
            }
        },
        /** Calls `thrower/t`'s `name`, and answers what it read of the error. */
        through: async (c, name: string, deadlineMs: number) => {
            try {
                await c.call(Address.of("thrower", "t"), name, [], {
                    deadlineMs,
                });
                return null;
            } catch (error) {
                return read(error);
            }
        },
    },
});

test("every code the all-local host makes reaches a caller with its group, code, message and metadata", async () => {
    await inFolder(async (dir) => {
        const host = await startLocalHost(dir, { thrower, relay });
        const t = host.at("thrower", "t");
        const r = host.at("relay", "r");
        await host.create(t);
        await host.create(r);
        const wrong = {
            group: "message",
            code: "wrong_caller",
            message: "This caller may not send 'wrongCaller'.",
            metadata: { action: "wrongCaller", n: 2 ** 40, list: [1, "x"] },
        };
        assert.deepEqual(
            await host.call(r, "through", ["wrongCaller", 5_000], CALL),
            wrong,
        );
        assert.deepEqual(
            read(
                await host
                    .call(t, "wrongCaller", [], CALL)
                    .catch((error: unknown) => error),
            ),
            wrong,
        );
        assert.deepEqual(
            await host.call(r, "through", ["noRoom", 5_000], CALL),
            {
                group: "user",
                code: "no_room",
                message: "no room",
                metadata: { n: 1 },
            },
        );
        const disk = await host.call(r, "through", ["diskFull", 5_000], CALL);
        assert.equal(
            `${disk?.group}.${disk?.code}`,
            "hamlet_actor.internal_error",
        );
        assert.ok(disk?.message.includes("the disk is full"), disk?.message);
        const late = await host.call(r, "through", ["slow", 200], CALL);
        assert.equal(`${late?.group}.${late?.code}`, "actor.action_timed_out");
        assert.deepEqual(
            await host.call(r, "through", ["nothing", 5_000], CALL),
            {
                group: "actor",
                code: "action_not_found",
                message: "Action `nothing` was not found.",
                metadata: null,
            },
        );
        for (const [to, options, code] of [
            [Address.of("nobody", "x"), CALL, "actor.not_registered"],
            [Address.of("thrower", "unmade"), CALL, "actor.not_found"],
            [t, { deadlineMs: 1.5 }, "napi.invalid_argument"],
            [t, { deadlineMs: 0 }, "napi.invalid_argument"],
        ] as const) {
            const error = read(
                await host
                    .call(to, "noRoom", [], options)
                    .catch((error: unknown) => error),
            );
            assert.equal(`${error.group}.${error.code}`, code);
        }
        assert.equal(
            existsSync(join(dir, "actors", "nobody", "x.sqlite")),
            false,
        );
        assert.equal(
            existsSync(join(dir, "actors", "thrower", "unmade.sqlite")),
            false,
        );
        await host.shutdown();
    });
});

test("a deadline that is not a whole number of milliseconds from 1 to 2^32 - 1 is refused, and the callee runs nothing", async () => {
    await inFolder(async (dir) => {
        const host = await startLocalHost(dir, { tally, relay });
        const t = host.at("tally", "t");
        const r = host.at("relay", "r");
        await host.create(t);
        await host.create(r);
        for (const deadlineMs of [
            0,
            -1,
            1.5,
            Number.NaN,
            Number.POSITIVE_INFINITY,
            2 ** 32,
        ]) {
            const viaActor = await host.call(r, "bump", [deadlineMs], CALL);
            assert.deepEqual(
                {
                    code: viaActor?.code,
                    argument: (viaActor?.metadata as { argument?: unknown })
                        ?.argument,
                },
                {
                    code: "napi.invalid_argument",
                    argument: "options.deadlineMs",
                },
                String(deadlineMs),
            );
            await assert.rejects(
                host.call(t, "bump", [], { deadlineMs }),
                (error) =>
                    isActorErrorCode(error, "napi", "invalid_argument") &&
                    (error.metadata as { argument?: unknown }).argument ===
                        "options.deadlineMs",
            );
        }
        assert.equal(await host.call(t, "count", [], CALL), 0);
        // 1 ms may time out, but is not refused.
        const one = await host.call(r, "bump", [1], CALL);
        assert.notEqual(one?.code, "napi.invalid_argument");
        assert.equal(await host.call(r, "bump", [2 ** 32 - 1], CALL), null);
        await host.shutdown();
    });
});
