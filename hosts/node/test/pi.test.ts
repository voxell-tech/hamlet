import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync, type SQLInputValue } from "node:sqlite";
import { test } from "node:test";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import {
    createModels,
    fauxAssistantMessage,
    fauxProvider,
    fauxText,
} from "@earendil-works/pi-ai";
import { createRegistry, Harness } from "@earendil-works/pi-durable";
import { SqliteStorage } from "@earendil-works/pi-durable/storage/sqlite";
import type { RawAccess } from "../src/actor/common/database/mod.ts";
import { actorSqlite } from "../src/pi/actor-sqlite.ts";

const context = BACKGROUND_CONTEXT;

/**
 * The actor layer's database client over a plain `node:sqlite` file, with no
 * actor under it. Calls run one at a time, and while a transaction is open
 * every other call waits for it, as the actor's own client does.
 */
function plainDatabase(path: string): RawAccess {
    const db = new DatabaseSync(path);
    let last: Promise<unknown> = Promise.resolve();
    const inTurn = <T>(work: () => Promise<T> | T): Promise<T> => {
        const next = last.then(work, work);
        last = next.catch(() => undefined);
        return next;
    };
    const run = <TRow>(sql: string, args: unknown[]): TRow[] => {
        const statement = db.prepare(sql);
        if (statement.columns().length > 0) {
            return statement.all(...(args as SQLInputValue[])) as TRow[];
        }
        if (args.length === 0) {
            // `prepare` compiles only the first statement, so a script with
            // no rows to return runs whole.
            db.exec(sql);
        } else {
            statement.run(...(args as SQLInputValue[]));
        }
        return [];
    };
    const inTransaction: RawAccess = {
        execute: async (sql, ...args) => run(sql, args),
        transaction: () => {
            throw new Error("nested transaction");
        },
        close: async () => {},
    };
    return {
        execute: (sql, ...args) => inTurn(() => run(sql, args)),
        transaction: (callback) =>
            inTurn(async () => {
                db.exec("BEGIN IMMEDIATE");
                try {
                    const result = await callback(inTransaction);
                    db.exec("COMMIT");
                    return result;
                } catch (error) {
                    db.exec("ROLLBACK");
                    throw error;
                }
            }),
        close: async () => db.close(),
    };
}

/** A pi-durable Harness on the wrapper's facade over `file`, with one faux model. */
async function openAgent(file: string) {
    const database = plainDatabase(file);
    const faux = fauxProvider();
    const models = createModels();
    models.setProvider(faux.provider);
    const storage = await SqliteStorage.open(actorSqlite(database));
    const harness = await Harness.open(
        storage,
        { models, registry: createRegistry() },
        context,
    );
    const model = faux.getModel();
    const root = await harness.root(context, {
        agent: { model: { provider: model.provider, modelId: model.id } },
    });
    const close = async () => {
        await harness.close(context);
        await database.close();
    };
    return { database, faux, root, close };
}

/** Every row of pi-durable's entries and submissions tables. */
async function transcript(database: RawAccess) {
    return {
        entries: await database.execute<{ kind: string }>(
            "SELECT *, record ->> '$.kind' AS kind FROM pi_entries ORDER BY id",
        ),
        submissions: await database.execute(
            "SELECT * FROM pi_submissions ORDER BY id",
        ),
    };
}

test("the pi wrapper's storage answers one prompt once on a plain SQLite file", async (t) => {
    const folder = mkdtempSync(join(tmpdir(), "hamlet-pi-"));
    t.after(() => rmSync(folder, { recursive: true, force: true }));
    const file = join(folder, "agent.sqlite");
    const prompt = {
        type: "input",
        content: "What is the capital of France?",
        requestId: "prompt-1",
    } as const;

    const first = await openAgent(file);
    first.faux.setResponses([fauxAssistantMessage([fauxText("Paris.")])]);
    const submission = await first.root.submit(prompt, context);
    const settled = await submission.wait(context);
    assert.equal(settled.status, "done");
    assert.equal(first.faux.state.callCount, 1);
    const answered = await transcript(first.database);
    const kinds = answered.entries.map((row) => row.kind);
    assert.deepEqual(
        kinds.filter((kind) => kind === "pi.user" || kind === "pi.assistant"),
        ["pi.user", "pi.assistant"],
    );
    assert.match(JSON.stringify(answered.entries), /Paris\./);

    const again = await first.root.submit(prompt, context);
    assert.equal(again.id, submission.id);
    assert.deepEqual(await again.wait(context), settled);
    assert.equal(first.faux.state.callCount, 1);
    assert.deepEqual(await transcript(first.database), answered);
    await first.close();

    // The request id is kept in the file, so a reopened agent still holds
    // the one answer and calls no model.
    const reopened = await openAgent(file);
    const afterReopen = await reopened.root.submit(prompt, context);
    assert.equal(afterReopen.id, submission.id);
    assert.deepEqual(await afterReopen.wait(context), settled);
    assert.equal(reopened.faux.state.callCount, 0);
    assert.deepEqual(await transcript(reopened.database), answered);
    await reopened.close();
});
