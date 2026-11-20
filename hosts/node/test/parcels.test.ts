import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import type { Context } from "@earendil-works/pi-ai";
import type { ParcelFile, Submit } from "../src/agent/messages/agent.ts";
import type { FetchPart, Part } from "../src/agent/messages/realm.ts";
import {
    asked,
    FAKE_ROOT,
    fakeCalls,
    fakeFiles,
    from,
    named,
    userWords,
    wakeAgent,
} from "./agent-testing.ts";

const INBOX = `${FAKE_ROOT}/inbox/from-hire-12`;

function letter(files: ParcelFile[]): Submit {
    return {
        id: "input-1",
        input: {
            type: "letter",
            from: { agent: 12, name: "Ada" },
            text: "The report",
            files,
        },
    };
}

function file(file: number, path: string, size: number): ParcelFile {
    return { parcel: 8, file, path: `from-hire-12/${path}`, size };
}

/** Answers each file of `held` in parts of `size` bytes. */
function serving(held: readonly string[], size: number) {
    return ({ file, part }: FetchPart): Part => {
        const text = held[file] ?? "";
        return {
            type: "bytes",
            bytes: Buffer.from(
                text.slice(part * size, (part + 1) * size),
            ).toString("base64"),
            last: (part + 1) * size >= text.length,
        };
    };
}

async function received(
    t: TestContext,
    held: Record<string, string>,
    serve: (message: FetchPart) => Part,
    submits: readonly Submit[],
) {
    const seen: Context[] = [];
    const { calls, sent } = fakeCalls({ realm: { fetch_part: serve } });
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(held),
        responses: submits.map(() => asked(seen, "Read.")),
    });
    for (const submit of submits) {
        await agent.actions.submit(agent.by(from.realm), submit);
        await agent.idle();
    }
    await agent.stop();
    return {
        words: seen.map((request) => userWords(request).at(-1)),
        fetched: named(sent, "fetch_part").map(({ message }) => message),
        reported: agent.reported,
    };
}

test("a letter's files land in the inbox and are told after its words, as today", async (t) => {
    const held: Record<string, string> = {};
    const { words, fetched } = await received(
        t,
        held,
        serving(["# Report\ndone\n", "png!", "a,b\n"], 9),
        [
            letter([
                file(0, "report.md", 14),
                file(1, "figures/plot.png", 4),
                file(2, "figures/data.csv", 4),
            ]),
        ],
    );

    // As `crates/hamlet_server/src/inbox.rs` `placed_captions` words them.
    assert.deepEqual(words, [
        `The report\n\n[File 1 of this message, 14 bytes, saved at ${INBOX}/report.md]\n[Folder 2 of this message, 2 files, 8 bytes, saved at ${INBOX}/figures]`,
    ]);
    assert.deepEqual(held, {
        "inbox/from-hire-12/report.md": "# Report\ndone\n",
        "inbox/from-hire-12/figures/plot.png": "png!",
        "inbox/from-hire-12/figures/data.csv": "a,b\n",
    });
    assert.deepEqual(fetched, [
        { parcel: 8, file: 0, part: 0 },
        { parcel: 8, file: 0, part: 1 },
        { parcel: 8, file: 1, part: 0 },
        { parcel: 8, file: 2, part: 0 },
    ]);
});

test("a name the inbox holds already is saved numbered, as today", async (t) => {
    const held: Record<string, string> = {
        "inbox/from-hire-12/report.md": "old",
        "inbox/from-hire-12/figures/old.png": "old",
    };
    const { words } = await received(t, held, serving(["new", "png!"], 9), [
        letter([file(0, "report.md", 3), file(1, "figures/plot.png", 4)]),
    ]);

    assert.deepEqual(words, [
        `The report\n\n[File 1 of this message, 3 bytes, saved at ${INBOX}/report-2.md]\n[Folder 2 of this message, 1 files, 4 bytes, saved at ${INBOX}/figures-2]`,
    ]);
    assert.equal(held["inbox/from-hire-12/report.md"], "old");
    assert.equal(held["inbox/from-hire-12/report-2.md"], "new");
    assert.equal(held["inbox/from-hire-12/figures-2/plot.png"], "png!");
});

test("a repeat of a letter it took fetches and saves nothing", async (t) => {
    const held: Record<string, string> = {};
    const sent = letter([file(0, "report.md", 3)]);
    const { words, fetched } = await received(t, held, serving(["new"], 9), [
        sent,
        sent,
    ]);

    assert.equal(words.length, 1);
    assert.equal(fetched.length, 1);
    assert.deepEqual(Object.keys(held), ["inbox/from-hire-12/report.md"]);
});

test("a file over 32 MiB is fetched and saved whole", async (t) => {
    const held: Record<string, string> = {};
    const big = "a".repeat(32 * 1024 * 1024 + 1);
    const { words, reported } = await received(
        t,
        held,
        serving([big], 720 * 1024),
        [letter([file(0, "big.bin", big.length)])],
    );

    assert.deepEqual(words, [
        `The report\n\n[File 1 of this message, 32.0 MiB, saved at ${INBOX}/big.bin]`,
    ]);
    assert.equal(held["inbox/from-hire-12/big.bin"]?.length, big.length);
    assert.deepEqual(reported, []);
});

test("a file over the workspace's 256 MiB is not fetched, and is reported", async (t) => {
    const held: Record<string, string> = {};
    const { words, fetched, reported } = await received(
        t,
        held,
        serving(["new"], 9),
        [letter([file(0, "big.bin", 256 * 1024 * 1024 + 1)])],
    );

    assert.deepEqual(words, ["The report"]);
    assert.deepEqual(fetched, []);
    assert.deepEqual(held, {});
    assert.equal(reported.length, 1);
    assert.match(reported[0] ?? "", /from-hire-12\/big\.bin/);
});

test("two files of one name are both saved, the second numbered, as today", async (t) => {
    const held: Record<string, string> = {};
    const { words } = await received(t, held, serving(["first", "second"], 9), [
        letter([file(0, "x.txt", 5), file(1, "x.txt", 6)]),
    ]);

    assert.deepEqual(held, {
        "inbox/from-hire-12/x.txt": "first",
        "inbox/from-hire-12/x-2.txt": "second",
    });
    // Today counts the second into the first's caption (`inbox.rs` `put_at`).
    assert.deepEqual(words, [
        `The report\n\n[File 1 of this message, 11 bytes, saved at ${INBOX}/x.txt]`,
    ]);
});

test("a size half way between two tenths rounds to the even one, as today", async (t) => {
    const held: Record<string, string> = {};
    const { words } = await received(
        t,
        held,
        serving(["a".repeat(1280), "a".repeat(1792)], 2048),
        [letter([file(0, "x.txt", 1280), file(1, "y.txt", 1792)])],
    );

    // Rust's `{:.1}` writes 1.25 as 1.2 and 1.75 as 1.8.
    assert.deepEqual(words, [
        `The report\n\n[File 1 of this message, 1.2 KiB, saved at ${INBOX}/x.txt]\n[File 2 of this message, 1.8 KiB, saved at ${INBOX}/y.txt]`,
    ]);
});

test("a part the realm does not answer leaves the file out, and the letter is still told", async (t) => {
    const held: Record<string, string> = {};
    const { words, reported } = await received(
        t,
        held,
        () => {
            throw new Error("the realm is gone");
        },
        [letter([file(0, "report.md", 3)])],
    );

    assert.deepEqual(words, ["The report"]);
    assert.deepEqual(held, {});
    assert.equal(reported.length, 1);
    assert.match(
        reported[0] ?? "",
        /from-hire-12\/report\.md.*the realm is gone/,
    );
});

test("a repeat that comes while the first is saved fetches and saves nothing", async (t) => {
    const held: Record<string, string> = {};
    const { calls, sent } = fakeCalls({
        realm: { fetch_part: serving(["new"], 9) },
    });
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(held),
        responses: [asked([], "Read.")],
    });
    const twice = letter([file(0, "report.md", 3)]);
    await Promise.all([
        agent.actions.submit(agent.by(from.realm), twice),
        agent.actions.submit(agent.by(from.realm), twice),
    ]);
    await agent.idle();
    await agent.stop();

    assert.equal(named(sent, "fetch_part").length, 1);
    assert.deepEqual(Object.keys(held), ["inbox/from-hire-12/report.md"]);
});

test("a question's files are told after its words, as a letter's are", async (t) => {
    const held: Record<string, string> = {};
    const { words } = await received(t, held, serving(["new"], 9), [
        {
            id: "input-1",
            input: {
                type: "question",
                letter: 6,
                from: { agent: 12, name: "Ada" },
                text: "Is this plan right?",
                files: [file(0, "plan.md", 3)],
            },
        },
    ]);

    assert.deepEqual(words, [
        `Is this plan right?\n\n[File 1 of this message, 3 bytes, saved at ${INBOX}/plan.md]`,
    ]);
});
