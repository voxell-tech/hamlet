import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import { ok } from "@earendil-works/pi-durable/env";
import type { AgentFilesystem } from "../src/agent/filesystem.ts";
import type { Skill } from "../src/agent/messages/library.ts";
import type { FilesAnswer } from "../src/agent/messages/profile.ts";
import {
    fakeCalls,
    fakeFiles,
    from,
    modelFailed,
    named,
    toolCall,
    until,
    wakeAgent,
} from "./agent-testing.ts";
import {
    A,
    B,
    bytes,
    filled,
    part,
    profileAnswering,
    readPrompt,
    saves,
    say,
} from "./branch-testing.ts";
import { fixture } from "./testing.ts";

test("a fill writes every file at the profile's tip, from every part, keeps its character, and holds the tip; the same fill again sends nothing", async (t) => {
    const { calls, sent } = fakeCalls({
        profile: profileAnswering(
            part({
                tip: A,
                character: { version: 3, text: "You write careful tests." },
                files: [
                    {
                        path: "memory/tests.md",
                        offset: 0,
                        bytes: bytes("# Notes\n"),
                        last: true,
                    },
                    {
                        path: "skills/run/run.sh",
                        offset: 0,
                        bytes: bytes("#!/bin/sh\n"),
                        last: false,
                    },
                ],
                next: { commit: A, file: 1, offset: 10 },
            }),
            part({
                tip: A,
                files: [
                    {
                        path: "skills/run/run.sh",
                        offset: 10,
                        bytes: bytes("echo run\n"),
                        last: true,
                    },
                ],
            }),
        ),
    });
    const files: Record<string, string> = {};
    const agent = await wakeAgent(t, { calls, files: fakeFiles(files) });

    assert.equal(
        await agent.actions.fill(agent.by(from.realm), { profile: 2 }),
        null,
    );
    assert.equal(
        await agent.actions.fill(agent.by(from.realm), { profile: 2 }),
        null,
    );

    assert.deepEqual(files, {
        "memory/tests.md": "# Notes\n",
        "skills/run/run.sh": "#!/bin/sh\necho run\n",
    });
    assert.deepEqual(
        named(sent, "files").map((call) => [call.to, call.message]),
        [
            ["profile/2", {}],
            ["profile/2", { after: { commit: A, file: 1, offset: 10 } }],
        ],
    );
    assert.equal(await filled(agent), A);
    assert.ok(
        (await readPrompt(agent)).includes(
            "# Your character\nYou write careful tests.\n",
        ),
    );
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("after a turn, a save sends what the filesystem changed since the fill on the filled commit, then pulls what the branch holds since it", async (t) => {
    const { calls, sent } = fakeCalls({
        profile: profileAnswering(
            part({
                tip: A,
                files: [
                    {
                        path: "memory/tests.md",
                        offset: 0,
                        bytes: bytes("# Notes\n"),
                        last: true,
                    },
                    {
                        path: "memory/old.md",
                        offset: 0,
                        bytes: bytes("old\n"),
                        last: true,
                    },
                ],
            }),
            part({ tip: B }),
        ),
    });
    const files: Record<string, string> = {};
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(files),
        responses: [fauxAssistantMessage("Saved.")],
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    files["memory/tests.md"] = "# Tests\n";
    delete files["memory/old.md"];
    files["CHARACTER.md"] = "Kept by the profile.";
    files["inbox/1.md"] = "A letter.";

    await say(agent, "Go.");

    assert.deepEqual(
        saves(sent).map(({ call: _, ...save }) => save),
        [
            {
                base: A,
                part: 0,
                last: true,
                changes: [
                    {
                        type: "write",
                        path: "memory/tests.md",
                        offset: 0,
                        bytes: bytes("# Tests\n"),
                        last: true,
                    },
                    { type: "remove", path: "memory/old.md" },
                ],
            },
        ],
    );
    assert.deepEqual(named(sent, "files").at(-1)?.message, { since: A });
    assert.equal(await filled(agent), B);
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

/** `fakeFiles` that keeps which files run, as a filesystem with modes does. */
function filesThatRun(files: Record<string, string>) {
    const fake = fakeFiles(files);
    const runs = new Set<string>();
    const filesystem: AgentFilesystem = {
        ...fake,
        write: async (path, content, context, executable) => {
            if (executable === true) runs.add(path);
            if (executable === false) runs.delete(path);
            return fake.write(path, content, context);
        },
        walk: async (path, context) => {
            const walked = await fake.walk(path, context);
            if (!walked.ok) return walked;
            const executable = walked.value.files.filter((file) =>
                runs.has(file),
            );
            return ok({ ...walked.value, executable });
        },
    };
    return { filesystem, runs };
}

test("a fill and a pull write whether each file runs, and a save sends it for each changed file that runs", async (t) => {
    const { calls, sent } = fakeCalls({
        profile: profileAnswering(
            part({
                tip: A,
                files: [
                    {
                        path: "skills/run/run.sh",
                        offset: 0,
                        bytes: bytes("#!/bin/sh\n"),
                        last: true,
                        executable: true,
                    },
                    {
                        path: "memory/tests.md",
                        offset: 0,
                        bytes: bytes("# Notes\n"),
                        last: true,
                    },
                ],
            }),
            part({
                tip: B,
                files: [
                    {
                        path: "skills/run/run.sh",
                        offset: 0,
                        bytes: bytes("#!/bin/sh\necho run\n"),
                        last: true,
                    },
                ],
            }),
        ),
    });
    const files: Record<string, string> = {};
    const { filesystem, runs } = filesThatRun(files);
    const agent = await wakeAgent(t, {
        calls,
        files: filesystem,
        responses: [fauxAssistantMessage("Saved.")],
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    assert.deepEqual([...runs], ["skills/run/run.sh"]);
    files["skills/run/run.sh"] = "#!/bin/sh\necho\n";
    files["memory/tests.md"] = "# Tests\n";

    await say(agent, "Go.");

    assert.deepEqual(saves(sent)[0]?.changes, [
        {
            type: "write",
            path: "skills/run/run.sh",
            offset: 0,
            bytes: bytes("#!/bin/sh\necho\n"),
            last: true,
            executable: true,
        },
        {
            type: "write",
            path: "memory/tests.md",
            offset: 0,
            bytes: bytes("# Tests\n"),
            last: true,
        },
    ]);
    assert.deepEqual([...runs], [], "a pull of a file that does not run");
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("a turn that changed no file sends no save", async (t) => {
    const { calls, sent } = fakeCalls({
        profile: profileAnswering(part({ tip: A })),
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [fauxAssistantMessage("Nothing.")],
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    await say(agent, "Go.");
    assert.deepEqual(saves(sent), []);
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("a file larger than a part is saved in parts of one call, numbered from 0, the last one last, which join to the file", async (t) => {
    const { calls, sent } = fakeCalls({
        profile: profileAnswering(part({ tip: A }), part({ tip: B })),
    });
    const files: Record<string, string> = {};
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(files),
        responses: [fauxAssistantMessage("Saved.")],
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    const large = "a".repeat(800 * 1024);
    files["memory/large.md"] = large;

    await say(agent, "Go.");

    const sentParts = saves(sent);
    assert.deepEqual(
        sentParts.map(({ part, last }) => [part, last]),
        [
            [0, false],
            [1, true],
        ],
    );
    assert.equal(new Set(sentParts.map(({ call }) => call)).size, 1);
    const pieces = sentParts.flatMap(({ changes }) => changes);
    assert.deepEqual(
        pieces.map((piece) =>
            piece.type === "write" ? [piece.offset, piece.last] : [],
        ),
        [
            [0, false],
            [
                Buffer.from(
                    pieces[0]?.type === "write" ? pieces[0].bytes : "",
                    "base64",
                ).length,
                true,
            ],
        ],
    );
    assert.equal(
        Buffer.concat(
            pieces.map((piece) =>
                Buffer.from(
                    piece.type === "write" ? piece.bytes : "",
                    "base64",
                ),
            ),
        ).toString(),
        large,
    );
    await agent.stop();
});

test("a move between turns is answered before its pull, keeps its character and pulls what changed since the filled commit", async (t) => {
    const pulled = Promise.withResolvers<FilesAnswer>();
    const asked = profileAnswering(part({ tip: A }));
    const { calls, sent } = fakeCalls({
        profile: {
            ...asked,
            files: () =>
                named(sent, "files").length === 1
                    ? asked.files()
                    : pulled.promise,
        },
    });
    const files: Record<string, string> = {};
    const agent = await wakeAgent(t, { calls, files: fakeFiles(files) });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });

    assert.equal(
        await agent.actions.moved(agent.by(from.profile(2)), {
            tip: B,
            character: { version: 4, text: "You merge with care." },
        }),
        null,
    );
    pulled.resolve(
        part({
            tip: B,
            files: [
                {
                    path: "memory/merged.md",
                    offset: 0,
                    bytes: bytes("Merged.\n"),
                    last: true,
                },
            ],
        }),
    );
    await until(() => files["memory/merged.md"] !== undefined);

    assert.deepEqual(named(sent, "files").at(-1)?.message, { since: A });
    assert.equal(files["memory/merged.md"], "Merged.\n");
    assert.equal(await filled(agent), B);
    assert.ok(
        (await readPrompt(agent)).includes(
            "# Your character\nYou merge with care.\n",
        ),
    );
    await agent.stop();
});

test("a move or a removal during a turn waits for the turn to end, and a removal answers later", async (t) => {
    const told = Promise.withResolvers<{ type: "done" }>();
    const asked = Promise.withResolvers<void>();
    const { calls, sent } = fakeCalls({
        profile: profileAnswering(part({ tip: A }), part({ tip: B })),
        realm: {
            tell: () => {
                asked.resolve();
                return told.promise;
            },
        },
    });
    const files: Record<string, string> = { "memory/password.md": "hunter2" };
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(files),
        responses: [
            toolCall("tell", { to: "mayor", text: "Working." }),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    await agent.actions.submit(agent.by(from.realm), {
        id: "line-1",
        input: { type: "line", text: "Go." },
    });
    await asked.promise;

    await agent.actions.moved(agent.by(from.profile(2)), { tip: B });
    assert.equal(
        await agent.actions.remove_path(agent.by(from.profile(2)), {
            path: "memory/password.md",
        }),
        fixture("profile/remove_path.later.answer"),
    );
    assert.equal(named(sent, "files").length, 1);
    assert.equal(files["memory/password.md"], "hunter2");

    told.resolve({ type: "done" });
    await agent.idle();
    await until(() => named(sent, "files").length === 2);
    assert.deepEqual(named(sent, "files").at(-1)?.message, { since: A });
    assert.equal(files["memory/password.md"], undefined);
    assert.deepEqual(saves(sent), []);
    await agent.stop();
});

test("a move or a removal that waits for a turn that fails is done once the turn ends", async (t) => {
    const told = Promise.withResolvers<{ type: "done" }>();
    const asked = Promise.withResolvers<void>();
    const { calls, sent } = fakeCalls({
        profile: profileAnswering(part({ tip: A }), part({ tip: B })),
        realm: {
            tell: () => {
                asked.resolve();
                return told.promise;
            },
        },
    });
    const files: Record<string, string> = { "memory/password.md": "hunter2" };
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(files),
        responses: [
            toolCall("tell", { to: "mayor", text: "Working." }),
            modelFailed("the model failed"),
        ],
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    await agent.actions.submit(agent.by(from.realm), {
        id: "line-1",
        input: { type: "line", text: "Go." },
    });
    await asked.promise;
    await agent.actions.moved(agent.by(from.profile(2)), { tip: B });
    assert.equal(
        await agent.actions.remove_path(agent.by(from.profile(2)), {
            path: "memory/password.md",
        }),
        fixture("profile/remove_path.later.answer"),
    );

    told.resolve({ type: "done" });
    await agent.idle();
    await until(
        () =>
            files["memory/password.md"] === undefined &&
            named(sent, "files").length === 2,
    );
    assert.deepEqual(named(sent, "files").at(-1)?.message, { since: A });
    await agent.stop();
});

test("a removal while a save is under way answers later, and removes the path once the save ends", async (t) => {
    const saved = Promise.withResolvers<{ type: "saved"; tip: string }>();
    const { calls, sent } = fakeCalls({
        profile: {
            ...profileAnswering(part({ tip: A }), part({ tip: B })),
            save: () => saved.promise,
        },
    });
    const files: Record<string, string> = { "memory/password.md": "hunter2" };
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(files),
        responses: [fauxAssistantMessage("Saved.")],
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    await agent.actions.submit(agent.by(from.realm), {
        id: "line-1",
        input: { type: "line", text: "Go." },
    });
    await until(() => named(sent, "save").length === 1);

    assert.equal(
        await agent.actions.remove_path(agent.by(from.profile(2)), {
            path: "memory/password.md",
        }),
        fixture("profile/remove_path.later.answer"),
    );
    assert.equal(files["memory/password.md"], "hunter2");
    saved.resolve({ type: "saved", tip: B });
    await until(() => files["memory/password.md"] === undefined);
    await agent.idle();
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("a removal between turns removes the path and answers removed", async (t) => {
    const { calls } = fakeCalls({
        profile: profileAnswering(part({ tip: A })),
    });
    const files: Record<string, string> = { "memory/password.md": "hunter2" };
    const agent = await wakeAgent(t, { calls, files: fakeFiles(files) });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });

    assert.equal(
        await agent.actions.remove_path(agent.by(from.profile(2)), {
            path: "memory/password.md",
        }),
        fixture("profile/remove_path.removed.answer"),
    );
    assert.equal(files["memory/password.md"], undefined);
    await agent.stop();
});

test("a move or a removal from a profile the agent was not filled from is refused", async (t) => {
    const { calls } = fakeCalls({
        profile: profileAnswering(part({ tip: A })),
    });
    const agent = await wakeAgent(t, { calls });
    for (const by of [from.profile(2), from.realm]) {
        await assert.rejects(
            agent.actions.moved(agent.by(by), { tip: B }),
            /may not send this message/,
        );
    }
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    await assert.rejects(
        agent.actions.remove_path(agent.by(from.profile(3)), { path: "a" }),
        /may not send this message/,
    );
    await agent.stop();
});

const LIBRARY_FOLDER = "/realm/library";

function shelf(name: string, description: string): Skill {
    return {
        name,
        description,
        usage: `Run skills/${name}/run.sh`,
    };
}

test("the shelves read at the wake and the shelves the library sends after reach the prompt's library section, and a wake asks for what changed since the version it holds", async (t) => {
    const { calls, sent } = fakeCalls({
        library: {
            shelves: () => ({
                version: 4,
                skills: [shelf("fixtures", "Writes the fixtures of a message")],
            }),
        },
    });
    const agent = await wakeAgent(t, { calls, library: LIBRARY_FOLDER });
    const section = (skill: Skill) =>
        `# The library\nSkills every agent shares, kept in ${LIBRARY_FOLDER}. Read one and run it from there; do not copy it into your lodge. You cannot write there. To share a skill of your own, use \`submit_skill\`.\n- ${skill.name}: ${skill.description} Run: \`Run ${LIBRARY_FOLDER}/${skill.name}/run.sh\` (${LIBRARY_FOLDER}/${skill.name}/SKILL.md)`;
    const first = shelf("fixtures", "Writes the fixtures of a message");
    await until(async () => (await readPrompt(agent)).includes(section(first)));

    const second = shelf("lint-docs", "Checks every link");
    assert.equal(
        await agent.actions.shelves_changed(agent.by(from.library), {
            list: { version: 5, skills: [second] },
        }),
        null,
    );
    const prompt = await readPrompt(agent);
    assert.ok(prompt.includes(section(second)), prompt);
    assert.ok(!prompt.includes("fixtures"), prompt);

    await agent.stop();
    const again = await agent.again();
    await until(() => named(sent, "shelves").length === 2);
    assert.deepEqual(
        named(sent, "shelves").map((call) => call.message),
        [{}, { version_seen: 5 }],
    );
    assert.deepEqual(again.reported, []);
    await again.stop();
});
