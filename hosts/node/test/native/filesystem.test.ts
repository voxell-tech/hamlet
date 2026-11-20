import assert from "node:assert/strict";
import {
    existsSync,
    mkdirSync,
    readFileSync,
    statSync,
    symlinkSync,
    writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import type { AgentFilesystem } from "../../src/agent/filesystem.ts";
import type { FilesAnswer, Piece } from "../../src/agent/messages/profile.ts";
import { openAgentFilesystem } from "../../src/agent/native-filesystem.ts";
import { fakeCalls, from, until, wakeAgent } from "../agent-testing.ts";
import { context, fixture, scratchFolder } from "../testing.ts";

async function walked(filesystem: AgentFilesystem) {
    const walk = await filesystem.walk("", context);
    assert.ok(walk.ok);
    return walk.value;
}

async function names(filesystem: AgentFilesystem, path: string) {
    const listed = await filesystem.list(path, context);
    assert.ok(listed.ok, path);
    return listed.value.map((entry) => entry.name);
}

async function written(
    filesystem: AgentFilesystem,
    files: Record<string, string>,
) {
    for (const [path, content] of Object.entries(files)) {
        assert.ok((await filesystem.write(path, content, context)).ok, path);
    }
}

test("the native filesystem removes a file or a folder with all it holds, and the folders it empties but the agent's own", async (t) => {
    const filesystem = openAgentFilesystem(scratchFolder(t), 1);
    await written(filesystem, {
        "skills/run/bin/run.sh": "run",
        "skills/run/SKILL.md": "skill",
        "memory/notes.md": "notes",
    });

    assert.ok((await filesystem.remove("skills/run/bin/run.sh", context)).ok);
    assert.deepEqual(await names(filesystem, "skills/run"), ["SKILL.md"]);
    await written(filesystem, {
        "skills/run/bin": "a file where a folder was",
    });
    assert.ok((await filesystem.remove("skills/run", context)).ok);
    assert.deepEqual((await walked(filesystem)).files, ["memory/notes.md"]);
    assert.ok((await filesystem.remove("memory/notes.md", context)).ok);
    assert.deepEqual(await names(filesystem, ""), [
        "inbox",
        "memory",
        "skills",
    ]);

    const missing = await filesystem.remove("memory/notes.md", context);
    assert.equal(missing.ok ? undefined : missing.error.code, "not_found");
    assert.equal((await filesystem.remove("", context)).ok, false);
});

test("a native removal through a link fails and leaves what the link names, and a link is removed itself", async (t) => {
    const realm = scratchFolder(t);
    const outside = join(realm, "outside");
    mkdirSync(outside);
    writeFileSync(join(outside, "secret"), "SECRET");
    const filesystem = openAgentFilesystem(realm, 1);
    symlinkSync(outside, join(filesystem.root, "skills", "out"));

    assert.equal(
        (await filesystem.remove("skills/out/secret", context)).ok,
        false,
    );
    const leaves = await filesystem.remove("../../../outside/secret", context);
    assert.equal(
        leaves.ok ? undefined : leaves.error.code,
        "permission_denied",
    );
    assert.ok((await filesystem.remove("skills/out", context)).ok);
    assert.equal(existsSync(join(filesystem.root, "skills", "out")), false);
    assert.equal(readFileSync(join(outside, "secret"), "utf8"), "SECRET");
});

test("a native write sets whether a file runs or keeps it, and the walk says which files run", {
    skip: process.platform === "win32" && "Windows keeps no mode that runs",
}, async (t) => {
    const filesystem = openAgentFilesystem(scratchFolder(t), 1);
    const runs = (path: string) =>
        (statSync(join(filesystem.root, path)).mode & 0o100) !== 0;
    assert.ok(
        (await filesystem.write("skills/run.sh", "#!/bin/sh\n", context, true))
            .ok,
    );
    await written(filesystem, { "memory/notes.md": "notes" });
    assert.equal(runs("skills/run.sh"), true);
    assert.equal(runs("memory/notes.md"), false);
    assert.deepEqual((await walked(filesystem)).executable, ["skills/run.sh"]);

    await written(filesystem, { "skills/run.sh": "#!/bin/sh\necho\n" });
    const edited = await filesystem.edit(
        "skills/run.sh",
        [{ old: "echo", new: "echo run" }],
        context,
    );
    assert.ok(edited.ok);
    assert.equal(runs("skills/run.sh"), true);

    assert.ok(
        (await filesystem.write("skills/run.sh", "plain", context, false)).ok,
    );
    assert.equal(runs("skills/run.sh"), false);
    assert.deepEqual((await walked(filesystem)).executable, []);
});

function piece(path: string, text: string, executable?: boolean): Piece {
    return {
        path,
        offset: 0,
        bytes: Buffer.from(text).toString("base64"),
        last: true,
        ...(executable === undefined ? {} : { executable }),
    };
}

test("a native agent fills a file that runs, removes what a pull removes, and removes a folder its profile asks it to", async (t) => {
    const A = "ab".repeat(20);
    const B = "cd".repeat(20);
    const answers: FilesAnswer[] = [
        {
            type: "part",
            tip: A,
            files: [
                piece("skills/run/run.sh", "#!/bin/sh\n", true),
                piece("skills/old/SKILL.md", "old"),
                piece("memory/notes.md", "notes"),
            ],
            removed: [],
        },
        {
            type: "part",
            tip: B,
            files: [],
            removed: [{ path: "skills/old/SKILL.md" }],
        },
    ];
    const { calls } = fakeCalls({
        profile: {
            files: () => {
                const next = answers.shift();
                assert.ok(next, "the profile was asked for more files");
                return next;
            },
            save: () => ({ type: "saved", tip: B }),
        },
    });
    const filesystem = openAgentFilesystem(scratchFolder(t), 1);
    const agent = await wakeAgent(t, { calls, files: filesystem });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    if (process.platform !== "win32") {
        assert.deepEqual((await walked(filesystem)).executable, [
            "skills/run/run.sh",
        ]);
    }

    await agent.actions.moved(agent.by(from.profile(2)), { tip: B });
    await until(
        async () => (await names(filesystem, "skills")).join() === "run",
    );
    assert.equal(
        await agent.actions.remove_path(agent.by(from.profile(2)), {
            path: "skills",
        }),
        fixture("profile/remove_path.removed.answer"),
    );
    assert.deepEqual((await walked(filesystem)).files, ["memory/notes.md"]);
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});
