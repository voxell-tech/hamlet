import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fauxAssistantMessage, fauxText } from "@earendil-works/pi-ai";
import type { RawAccess } from "../src/actor/common/database/mod.ts";
import { context, openAgent } from "./agent-fixture.ts";

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
