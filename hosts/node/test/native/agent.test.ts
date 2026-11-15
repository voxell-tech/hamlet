import assert from "node:assert/strict";
import {
    existsSync,
    mkdirSync,
    readdirSync,
    readFileSync,
    symlinkSync,
    writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { type TestContext, test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import { openAgentFilesystem } from "../../src/agent/native-filesystem.ts";
import {
    fakeCalls,
    from,
    toolCall,
    toolResults,
    wakeAgent,
} from "../agent-testing.ts";
import { context, scratchFolder } from "../testing.ts";

const decoder = new TextDecoder();

/**
 * Agent 1's filesystem in a scratch realm folder, with `memory/notes.md`,
 * beside a folder `outside` that holds `secret`.
 */
async function planted(t: TestContext) {
    const realm = scratchFolder(t);
    const outside = join(realm, "outside");
    mkdirSync(outside);
    writeFileSync(join(outside, "secret"), "SECRET");
    const filesystem = openAgentFilesystem(realm, 1);
    assert.ok((await filesystem.write("memory/notes.md", "notes", context)).ok);
    return { realm, outside, filesystem };
}

function outsideIsUntouched(outside: string) {
    assert.deepEqual(readdirSync(outside), ["secret"]);
    assert.equal(readFileSync(join(outside, "secret"), "utf8"), "SECRET");
}

test("the agent's filesystem is a folder beside the agent's database, with memory, skills and inbox", (t) => {
    const realm = scratchFolder(t);
    const filesystem = openAgentFilesystem(realm, 1);
    assert.equal(filesystem.root, join(realm, "actors", "agent", "1.fs"));
    assert.deepEqual(readdirSync(filesystem.root).sort(), [
        "inbox",
        "memory",
        "skills",
    ]);
});

test("a filesystem read gives a file's bytes, by a relative or an absolute path", async (t) => {
    const { filesystem } = await planted(t);
    for (const path of [
        "memory/notes.md",
        "skills/../memory/notes.md",
        `${filesystem.root}/memory/notes.md`,
    ]) {
        const read = await filesystem.read(path, context);
        assert.ok(read.ok, path);
        assert.equal(decoder.decode(read.value), "notes");
    }
    const missing = await filesystem.read("memory/missing.md", context);
    assert.equal(missing.ok ? undefined : missing.error.code, "not_found");
});

test("a filesystem write makes the file and its folders, then reads back", async (t) => {
    const { filesystem } = await planted(t);
    const wrote = await filesystem.write("memory/new/today.md", "new", context);
    assert.ok(wrote.ok);
    const again = await filesystem.write("memory/new/today.md", "new", context);
    assert.ok(again.ok);
    assert.equal(
        readFileSync(join(filesystem.root, "memory/new/today.md"), "utf8"),
        "new",
    );
    const read = await filesystem.read("memory/new/today.md", context);
    assert.equal(read.ok && decoder.decode(read.value), "new");
});

test("a filesystem path that tries to leave the folder is refused", async (t) => {
    const { realm, outside, filesystem } = await planted(t);
    symlinkSync(join(outside, "secret"), join(filesystem.root, "memory/link"));
    symlinkSync(outside, join(filesystem.root, "skills/out"));
    const refusals: [string, string][] = [
        ["../../../../outside/secret", "permission_denied"],
        ["memory/../../../../outside/secret", "permission_denied"],
        [join(outside, "secret"), "permission_denied"],
        ["memory/link", "invalid"],
        ["skills/out/secret", "not_directory"],
    ];
    for (const [path, code] of refusals) {
        const read = await filesystem.read(path, context);
        assert.equal(read.ok ? "read" : read.error.code, code, path);
        if (path !== "memory/link") {
            const wrote = await filesystem.write(path, "x", context);
            assert.ok(!wrote.ok, path);
        }
    }
    assert.ok(!(await filesystem.list("skills/out", context)).ok);
    assert.ok(!(await filesystem.write("../escape", "x", context)).ok);
    assert.ok(!existsSync(join(realm, "actors", "agent", "escape")));
    outsideIsUntouched(outside);
});

test("search and find never follow a link", async (t) => {
    const { outside, filesystem } = await planted(t);
    symlinkSync(
        join(outside, "secret"),
        join(filesystem.root, "memory/secret"),
    );
    symlinkSync(outside, join(filesystem.root, "skills/out"));
    symlinkSync(".", join(filesystem.root, "inbox/loop"));
    assert.ok(
        (await filesystem.write("skills/a/SKILL.md", "SECRET word", context))
            .ok,
    );

    const found = await filesystem.find("*", "", context);
    assert.deepEqual(found.ok && found.value, {
        found: [
            `${filesystem.root}/memory/notes.md`,
            `${filesystem.root}/skills/a/SKILL.md`,
        ],
        more: false,
        capped: false,
    });
    const byPath = await filesystem.find("a/*.md", "skills", context);
    assert.deepEqual(byPath.ok && byPath.value.found, [
        `${filesystem.root}/skills/a/SKILL.md`,
    ]);
    const searched = await filesystem.search("SECRET", "", undefined, context);
    assert.deepEqual(searched.ok && searched.value, {
        found: [
            {
                path: `${filesystem.root}/skills/a/SKILL.md`,
                line: 1,
                text: "SECRET word",
            },
        ],
        more: false,
        capped: false,
    });
    const listed = await filesystem.list("skills", context);
    assert.deepEqual(
        listed.ok && listed.value.map(({ name, kind }) => [name, kind]),
        [
            ["a", "directory"],
            ["out", "symlink"],
        ],
    );
    const pattern = await filesystem.search("(a", "", undefined, context);
    assert.equal(pattern.ok ? undefined : pattern.error.code, "invalid");
});

test("an edit replaces text found once, and a copy stays inside", async (t) => {
    const { filesystem } = await planted(t);
    const edited = await filesystem.edit(
        "memory/notes.md",
        [{ old: "notes", new: "more notes" }],
        context,
    );
    assert.ok(edited.ok);
    const twice = await filesystem.edit(
        "memory/notes.md",
        [{ old: "o", new: "0" }],
        context,
    );
    assert.equal(twice.ok ? undefined : twice.error.code, "invalid");
    assert.ok(
        (await filesystem.copy("memory/notes.md", "inbox/copy.md", context)).ok,
    );
    const read = await filesystem.read("inbox/copy.md", context);
    assert.equal(read.ok && decoder.decode(read.value), "more notes");
    assert.ok(
        !(await filesystem.copy("memory/notes.md", "../copy.md", context)).ok,
    );
});

test("a search with a name looks only in the files the name matches, before its bound", async (t) => {
    const { filesystem } = await planted(t);
    const many = Array.from({ length: 120 }, () => "hit").join("\n");
    for (const path of ["memory/a.txt", "memory/z.txt"]) {
        assert.ok((await filesystem.write(path, many, context)).ok);
    }
    assert.ok((await filesystem.write("memory/m.md", "one hit", context)).ok);
    const searched = await filesystem.search(
        "hit",
        "",
        { name: "*.md" },
        context,
    );
    assert.deepEqual(searched.ok && searched.value, {
        found: [
            {
                path: `${filesystem.root}/memory/m.md`,
                line: 1,
                text: "one hit",
            },
        ],
        more: false,
        capped: false,
    });
});

test("a path the file tools join to the root through `.` or `..` is read inside it or refused", async (t) => {
    const { outside, filesystem } = await planted(t);
    const read = await filesystem.read(
        `${filesystem.root}/./memory/notes.md`,
        context,
    );
    assert.equal(read.ok && decoder.decode(read.value), "notes");
    const listed = await filesystem.list(`${filesystem.root}/.`, context);
    assert.deepEqual(listed.ok && listed.value.map((entry) => entry.name), [
        "inbox",
        "memory",
        "skills",
    ]);
    const climbed = await filesystem.read(
        `${filesystem.root}/../../../outside/secret`,
        context,
    );
    assert.equal(climbed.ok ? "read" : climbed.error.code, "permission_denied");
    outsideIsUntouched(outside);
});

test("the root itself, as a left-out path reaches it, can be listed, found in and searched", async (t) => {
    const { filesystem } = await planted(t);
    const { root } = filesystem;
    const listed = await filesystem.list(root, context);
    assert.deepEqual(listed.ok && listed.value.map((entry) => entry.name), [
        "inbox",
        "memory",
        "skills",
    ]);
    const found = await filesystem.find("*.md", root, context);
    assert.deepEqual(found.ok && found.value.found, [
        `${root}/memory/notes.md`,
    ]);
    const searched = await filesystem.search("notes", root, undefined, context);
    assert.deepEqual(searched.ok && searched.value.found, [
        { path: `${root}/memory/notes.md`, line: 1, text: "notes" },
    ]);
});

test("an own edit the filesystem refuses answers in today's words, and the file stays", async (t) => {
    const { filesystem } = await planted(t);
    assert.ok(
        (await filesystem.write("memory/notes.md", "one two two\n", context))
            .ok,
    );
    const path = `${filesystem.root}/memory/notes.md`;
    const edits: [string, string][][] = [
        [],
        [["", "x"]],
        [
            ["one", "1"],
            ["", "x"],
        ],
        [["three", "3"]],
        [
            ["one", "1"],
            ["three", "3"],
        ],
        [["two", "2"]],
        [
            ["one", "1"],
            ["two", "2"],
        ],
        [
            ["one two", "a"],
            ["e t", "b"],
        ],
        [["one", "one"]],
        [
            ["one", "one"],
            ["two two", "two two"],
        ],
    ];
    const agent = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        files: filesystem,
        responses: [
            ...edits.map((each, at) =>
                toolCall(
                    "edit",
                    {
                        path: "memory/notes.md",
                        edits: each.map(([oldText, newText]) => ({
                            oldText,
                            newText,
                        })),
                    },
                    `c${at}`,
                ),
            ),
            fauxAssistantMessage("None took."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        input: { type: "line", text: "Edit." },
    });
    await agent.idle();

    // Today's `CallError` (`crates/hamlet_server/src/work/tools/files.rs`).
    const refused = (message: string) => ({
        output: "",
        isError: true,
        diagnostics: [{ severity: "error", code: "tool_error", message }],
    });
    assert.deepEqual(await toolResults(agent), [
        refused(
            "Edit tool input is invalid. edits must contain at least one replacement.",
        ),
        refused(`oldText must not be empty in ${path}.`),
        refused(`edits[1].oldText must not be empty in ${path}.`),
        refused(
            `Could not find the exact text in ${path}. The old text must match exactly including all whitespace and newlines.`,
        ),
        refused(
            `Could not find edits[1] in ${path}. The oldText must match exactly including all whitespace and newlines.`,
        ),
        refused(
            `Found 2 occurrences of the text in ${path}. The text must be unique. Please provide more context to make it unique.`,
        ),
        refused(
            `Found 2 occurrences of edits[1] in ${path}. Each oldText must be unique. Please provide more context to make it unique.`,
        ),
        refused(
            `edits[0] and edits[1] overlap in ${path}. Merge them into one edit or target disjoint regions.`,
        ),
        refused(
            `No changes made to ${path}. The replacement produced identical content. This might indicate an issue with special characters or the text not existing as expected.`,
        ),
        refused(
            `No changes made to ${path}. The replacements produced identical content.`,
        ),
    ]);
    assert.equal(readFileSync(path, "utf8"), "one two two\n");
    await agent.stop();
});
