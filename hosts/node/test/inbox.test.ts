import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import { err, FileError } from "@earendil-works/pi-durable/env";
import type { AgentFilesystem } from "../src/agent/filesystem.ts";
import type {
    InboxCursor,
    InboxPart,
    ReadInbox,
} from "../src/agent/messages/agent.ts";
import {
    fakeCalls,
    fakeFiles,
    filesThatRun,
    from,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { fixture } from "./testing.ts";

const BIG = "x".repeat(800 * 1024);

/** An inbox with a file, one over a part, one that runs and nested paths. */
function absorbedInbox(): Record<string, string> {
    return {
        "inbox/from-chief/letter.md": "a letter\n",
        "inbox/from-chief/scan.pdf": BIG,
        "inbox/from-hire-9/tools/run.sh": "#!/bin/sh\n",
        "inbox/from-hire-9/a/b/c/deep.txt": "deep\n",
        "memory/notes.md": "# Notes\n",
    };
}

async function hire(
    t: TestContext,
    key: string,
    files: AgentFilesystem,
): Promise<Woken> {
    return wakeAgent(t, { calls: fakeCalls({}).calls, key, files });
}

/** One read, take and drop of the realm's merge step, from `after`. */
async function step(
    absorbed: Woken,
    kept: Woken,
    after: InboxCursor | undefined,
    drop = true,
): Promise<InboxPart> {
    const realm = (agent: Woken) => agent.by(from.realm);
    const part = await absorbed.actions.read_inbox(
        realm(absorbed),
        after === undefined ? {} : { after },
    );
    assert.equal(
        await kept.actions.take_inbox(realm(kept), {
            from: 4,
            files: part.files,
        }),
        null,
    );
    if (drop) {
        const paths = part.files
            .filter((piece) => piece.last)
            .map((piece) => piece.path);
        await absorbed.actions.drop_inbox(realm(absorbed), { paths });
    }
    return part;
}

async function moved(absorbed: Woken, kept: Woken): Promise<number> {
    let parts = 0;
    let after: InboxCursor | undefined;
    do {
        after = (await step(absorbed, kept, after)).next;
        parts += 1;
    } while (after !== undefined);
    return parts;
}

function expectedKept(): Record<string, string> {
    return {
        "inbox/from-hire-4/from-chief/letter.md": "a letter\n",
        "inbox/from-hire-4/from-chief/scan.pdf": BIG,
        "inbox/from-hire-4/from-hire-9/a/b/c/deep.txt": "deep\n",
        "inbox/from-hire-4/from-hire-9/tools/run.sh": "#!/bin/sh\n",
    };
}

test("a merge's inbox step moves every file under from-hire-<n>/, its executable bit kept, and gives the kept hire no turn", async (t) => {
    const absorbedFiles = absorbedInbox();
    const source = filesThatRun(absorbedFiles);
    source.runs.add("inbox/from-hire-9/tools/run.sh");
    const keptFiles: Record<string, string> = {};
    const target = filesThatRun(keptFiles);
    const absorbed = await hire(t, "4", source.filesystem);
    const kept = await hire(t, "3", target.filesystem);

    assert.ok((await moved(absorbed, kept)) > 1, "the big file spans parts");
    await kept.idle();
    const entries = await kept.entries();
    await absorbed.stop();
    await kept.stop();

    assert.deepEqual(keptFiles, expectedKept());
    assert.deepEqual(
        [...target.runs],
        ["inbox/from-hire-4/from-hire-9/tools/run.sh"],
    );
    assert.deepEqual(absorbedFiles, { "memory/notes.md": "# Notes\n" });
    assert.equal(kept.faux.state.callCount, 0);
    assert.deepEqual(entries, []);
    assert.deepEqual([...absorbed.reported, ...kept.reported], []);
});

test("a merge's inbox step driven again after a take and before its drop ends as one driven once", async (t) => {
    const absorbedFiles = absorbedInbox();
    const keptFiles: Record<string, string> = {
        "inbox/from-hire-4/from-chief/letter.md": "an older letter\n",
    };
    const absorbed = await hire(t, "4", fakeFiles(absorbedFiles));
    const kept = await hire(t, "3", fakeFiles(keptFiles));

    // The second part ends inside the big file, whose first piece is held.
    const first = await step(absorbed, kept, undefined, false);
    const second = await step(absorbed, kept, first.next, false);
    assert.equal(second.next?.path, "from-chief/scan.pdf");
    assert.notEqual(second.next?.offset, 0);
    await moved(absorbed, kept);
    await absorbed.stop();
    await kept.stop();

    assert.deepEqual(keptFiles, expectedKept());
    assert.deepEqual(absorbedFiles, { "memory/notes.md": "# Notes\n" });
});

test("a take refuses a piece that does not follow what it holds of its file", async (t) => {
    const keptFiles: Record<string, string> = {};
    const kept = await hire(t, "3", fakeFiles(keptFiles));

    await assert.rejects(
        kept.actions.take_inbox(kept.by(from.realm), {
            from: 4,
            files: [
                {
                    path: "scan.pdf",
                    offset: 5,
                    bytes: Buffer.from("tail").toString("base64"),
                    last: true,
                },
            ],
        }),
        /has 0 bytes before a piece at 5/,
    );
    await kept.stop();

    assert.deepEqual(keptFiles, {});
});

test("an agent with no inbox reads one empty part", async (t) => {
    const fake = fakeFiles({ "memory/notes.md": "# Notes\n" });
    const absorbed = await hire(t, "4", {
        ...fake,
        list: async (path) =>
            err(new FileError("not_found", `ENOENT: ${path}`, path)),
    });

    const part = await absorbed.actions.read_inbox(absorbed.by(from.realm), {});
    await absorbed.stop();

    assert.deepEqual(part, { files: [] });
});

test("agent/read_inbox's arguments and answers are the agent's", async (t) => {
    const scan = "y".repeat(737_216 + 4);
    const absorbed = await hire(
        t,
        "4",
        fakeFiles({
            "inbox/from-chief/letter.md": "a letter\n",
            "inbox/scan.pdf": scan,
        }),
    );
    const realm = absorbed.by(from.realm);
    const [first] = fixture("agent/read_inbox.args") as [ReadInbox];
    const [after] = fixture("agent/read_inbox.after.args") as [ReadInbox];
    const answer = fixture("agent/read_inbox.answer") as InboxPart;

    const whole = await absorbed.actions.read_inbox(realm, first);
    const rest = await absorbed.actions.read_inbox(realm, after);
    await absorbed.stop();

    // The fixture's cursor stands where its part was full; here the part
    // ends before scan.pdf.
    assert.deepStrictEqual(whole, {
        ...answer,
        next: { path: "scan.pdf", offset: 0 },
    });
    assert.deepStrictEqual(rest, {
        files: [
            {
                path: "scan.pdf",
                offset: 737_216,
                bytes: Buffer.from("yyyy").toString("base64"),
                last: true,
            },
        ],
    });
});

test("agent/read_inbox.last is the answer of an inbox that fits one part", async (t) => {
    const absorbed = await hire(
        t,
        "4",
        fakeFiles({ "inbox/from-chief/letter.md": "a letter\n" }),
    );

    const part = await absorbed.actions.read_inbox(absorbed.by(from.realm), {});
    await absorbed.stop();

    assert.deepStrictEqual(part, fixture("agent/read_inbox.last.answer"));
});
