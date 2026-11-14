import assert from "node:assert/strict";
import { test } from "node:test";
import {
    ActorDefinition,
    ActorError,
    actor,
    event,
    UserError,
} from "../src/actor/actor/mod.ts";
import { db } from "../src/actor/db/mod.ts";
import { migrations } from "../src/actor/unstable/migrations.ts";

test("actor defines an actor with its actions", () => {
    const greet = () => "hello";
    const definition = actor({ actions: { greet } });
    assert.ok(definition instanceof ActorDefinition);
    assert.equal(definition.config.actions?.greet, greet);
});

test("event returns its options as the event's token", () => {
    const options = { canSubscribe: () => true };
    assert.equal(event<string>(options), options);
    assert.deepEqual(event<string>(), {});
});

test("UserError is a public actor error in the user group", () => {
    const error = new UserError("no such room", { code: "no_room" });
    assert.ok(error instanceof ActorError);
    assert.equal(error.message, "no such room");
    assert.equal(error.group, "user");
    assert.equal(error.code, "no_room");
    assert.equal(error.public, true);
});

test("db needs the runtime's native database provider", async () => {
    const provider = db();
    await assert.rejects(
        provider.createClient({ actorId: "a" } as never),
        /native SQLite is required/,
    );
});

test("migrations applies only the steps past the stored version", async () => {
    const statements: string[] = [];
    const database = {
        execute: async (sql: string, ...args: unknown[]) => {
            statements.push(
                sql.trim().split(/\s+/).slice(0, 3).join(" ") +
                    (args.length ? ` ${args.join(",")}` : ""),
            );
            return (
                sql.startsWith("SELECT") ? [{ schema_version: 1 }] : []
            ) as never;
        },
    };
    const onMigrate = migrations({
        migrations: [
            { version: 1, sql: "CREATE TABLE one (x)" },
            { version: 2, sql: "CREATE TABLE two (x)" },
        ],
    });
    await onMigrate(database);
    assert.deepEqual(statements, [
        "CREATE TABLE IF",
        "SELECT schema_version FROM",
        "CREATE TABLE two",
        'INSERT INTO "actor_schema_version" 2',
    ]);
});
