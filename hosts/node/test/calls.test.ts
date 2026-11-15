import assert from "node:assert/strict";
import { test } from "node:test";
import { addressFromHost, callerOf } from "../src/actor/actor/calls.ts";
import {
    ActorError,
    Address,
    isActorErrorCode,
} from "../src/actor/actor/mod.ts";
import { callThrough } from "../src/actor/registry/native.ts";

test("a name that is not plain is refused before any call, with one code", () => {
    for (const [kind, key, refused] of [
        ["Bad Kind", "a", "Bad Kind"],
        ["agent", "", ""],
        ["agent", ".a", ".a"],
    ] as const) {
        assert.throws(
            () => Address.of(kind, key),
            (error) =>
                isActorErrorCode(error, "napi", "invalid_argument") &&
                error.message ===
                    `Invalid native argument 'to': ${(error.metadata as { reason: string }).reason}` &&
                (error.metadata as { argument: string }).argument === "to" &&
                (error.metadata as { reason: string }).reason.startsWith(
                    `\`${refused}\` is not a valid actor kind or key`,
                ),
        );
    }
    for (const name of ["a", "1", "a.b", "a-b_c", "a".repeat(128)]) {
        assert.equal(Address.of(name, name).key, name);
    }
    for (const name of ["Bad", "a b", "é", "a".repeat(129)]) {
        assert.throws(() => Address.of("agent", name), ActorError);
    }

    const address = Address.of("agent", "1");
    assert.ok(Object.isFrozen(address));
    assert.ok(Address.equals(address, addressFromHost("agent", "1")));
    assert.ok(!Address.equals(address, Address.of("agent", "2")));
    assert.ok(!Address.equals(address, Address.of("agents", "1")));
});

test("callerOf reads each form of the host's stamp", () => {
    assert.equal(callerOf(null), null);
    assert.equal(callerOf("client"), "client");
    assert.deepEqual(callerOf({ actor: { kind: "agent", key: "1" } }), {
        actor: { kind: "agent", key: "1" },
    });
    for (const stamp of [
        undefined,
        "actor",
        { actor: { kind: "agent" } },
        { client: null },
        { actor: { kind: 1, key: "1" } },
    ]) {
        assert.throws(
            () => callerOf(stamp),
            (error) => isActorErrorCode(error, "actor", "internal_error"),
            JSON.stringify(stamp),
        );
    }
});

// `a1616e1b0000010000000000` is the CBOR Rust writes for `{ n: 2^40 }`.
test("an answer's integer from 2^32 reaches the caller as a number", async () => {
    const answer = await callThrough(
        async () => Buffer.from("a1616e1b0000010000000000", "hex"),
        [],
    );
    assert.deepEqual(answer, { n: 2 ** 40 });
    assert.equal(typeof (answer as { n: unknown }).n, "number");
});
