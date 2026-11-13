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
import { openAgentFilesystem } from "../../src/agent/filesystem.ts";
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
