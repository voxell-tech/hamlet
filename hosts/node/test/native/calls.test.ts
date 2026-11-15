import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { addressFromHost } from "../../src/actor/actor/calls.ts";
import {
    type ActionsOf,
    ActorError,
    Address,
    actor,
    type CallOptions,
    isActorErrorCode,
} from "../../src/actor/actor/mod.ts";
import { db } from "../../src/actor/db/mod.ts";
import { inFolder, startLocalHost } from "./support.ts";

const CALL: CallOptions = { deadlineMs: 10_000 };

const answerer = actor({
    actions: {
        who: (c, said: Address) => ({
            me: c.address,
            caller: c.caller,
            saidIsCaller:
                c.caller !== null &&
                c.caller !== "client" &&
                Address.equals(c.caller.actor, said),
        }),
    },
});

const asker = actor({
    actions: {
        /** Calls `answerer/<key>` and tells it this actor's own address. */
        ask: async (c, key: string) =>
            await c.call(
                Address.of<ActionsOf<typeof answerer>>("answerer", key),
                "who",
                [c.address],
                CALL,
            ),
        askItself: async (c) => await c.call(c.address, "ask", ["b"], CALL),
    },
});

/** Calls `answerer/b` from its start hook, and keeps what it was told. */
const greeter = actor({
    db: db({
        onMigrate: async (raw) => {
            await raw.execute(
                "CREATE TABLE IF NOT EXISTS met (said TEXT NOT NULL)",
            );
        },
    }),
    onWake: async (c) => {
        const met = await c.call(
            Address.of<ActionsOf<typeof answerer>>("answerer", "b"),
            "who",
            [c.address],
            CALL,
        );
        await c.db.run(
            "INSERT INTO met (said) VALUES (?)",
            JSON.stringify(met.caller),
        );
    },
    actions: {
        met: async (c) =>
            await c.db.all<{ said: string }>("SELECT said FROM met"),
    },
});

test("an actor names another and itself, and the callee reads the caller the host stamped", async () => {
    await inFolder(async (dir) => {
        const host = await startLocalHost(dir, { asker, answerer });
        const a = host.at("asker", "a");
        const b = host.at("answerer", "b");
        await host.create(a);
        await host.create(b);

        assert.deepEqual(await host.call(a, "ask", ["b"], CALL), {
            me: { kind: "answerer", key: "b" },
            caller: { actor: { kind: "asker", key: "a" } },
            saidIsCaller: true,
        });
        assert.ok(existsSync(join(dir, "actors", "answerer", "b.sqlite")));

        // The program's call carries no caller, also right after an actor's.
        assert.deepEqual(await host.call(b, "who", [a], CALL), {
            me: { kind: "answerer", key: "b" },
            caller: null,
            saidIsCaller: false,
        });
        await assert.rejects(host.call(a, "ask", ["nobody"], CALL), (error) =>
            isActorErrorCode(error, "actor", "not_found"),
        );
        assert.equal(
            existsSync(join(dir, "actors", "answerer", "nobody.sqlite")),
            false,
        );
        await assert.rejects(host.call(a, "askItself", [], CALL), (error) =>
            isActorErrorCode(error, "actor", "invalid_operation"),
        );
        await host.shutdown();
    });
});

test("an actor calls another actor while it starts", async () => {
    await inFolder(async (dir) => {
        const host = await startLocalHost(dir, { answerer, greeter });
        await host.create(host.at("answerer", "b"));
        const g = host.at("greeter", "g");
        await host.create(g);
        assert.deepEqual(await host.call(g, "met", [], CALL), [
            { said: JSON.stringify({ actor: { kind: "greeter", key: "g" } }) },
        ]);
        await host.shutdown();
    });
});

/** Every field of an error a caller can read. */
function readAll(error: unknown) {
    assert.ok(error instanceof ActorError, String(error));
    const { group, code, message, metadata, public: shown, statusCode } = error;
    return { group, code, message, metadata, public: shown, statusCode };
}

test("Address.of and the native call accept and refuse the same names, with one error", async () => {
    await inFolder(async (dir) => {
        const host = await startLocalHost(dir, { answerer });
        const names = [
            ...Array.from({ length: 256 }, (_, code) =>
                String.fromCharCode(code),
            ),
            "",
            ".a",
            "a.",
            "a".repeat(128),
            "a".repeat(129),
            "Bad",
        ];
        for (const name of names) {
            let typed: unknown;
            try {
                Address.of(name, "k");
            } catch (error) {
                typed = error;
            }
            const native = await host
                .getOrStart(addressFromHost(name, "k"))
                .then(
                    () => undefined,
                    (error: unknown) => error,
                );
            assert.equal(
                typed !== undefined,
                isActorErrorCode(native, "napi", "invalid_argument"),
                JSON.stringify(name),
            );
            if (typed !== undefined) {
                assert.deepEqual(readAll(typed), readAll(native));
            }
        }
        await host.shutdown();
    });
});
