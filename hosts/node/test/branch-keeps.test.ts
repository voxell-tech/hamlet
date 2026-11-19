import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { SaveAnswer } from "../src/agent/messages/profile.ts";
import {
    cappedFiles,
    fakeCalls,
    fakeFiles,
    from,
    wakeAgent,
} from "./agent-testing.ts";
import {
    A,
    B,
    part,
    piece,
    profileAnswering,
    saves,
    say,
} from "./branch-testing.ts";
import { fixture } from "./testing.ts";

test("a save whose walk stops at its cap saves what it walked, and reports that files past the cap were not saved", async (t) => {
    const { calls, sent } = fakeCalls({
        profile: profileAnswering(part({ tip: A }), part({ tip: A })),
    });
    const files: Record<string, string> = {};
    const agent = await wakeAgent(t, {
        calls,
        files: cappedFiles(files),
        responses: [fauxAssistantMessage("Saved.")],
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    files["memory/a.md"] = "A.";

    await say(agent, "Go.");

    assert.deepEqual(
        saves(sent).flatMap(({ changes }) => changes.map(({ path }) => path)),
        ["memory/a.md"],
    );
    assert.deepEqual(agent.reported, [
        "the filesystem was saved in part: the walk stopped after 20000 names, so files past them were not saved",
    ]);
    await agent.stop();
});

test("a save whose answer is lost is sent again as it was, under its call, and an edit made since goes out after it under another", async (t) => {
    let lost = true;
    const { calls, sent } = fakeCalls({
        profile: {
            ...profileAnswering(
                part({ tip: A }),
                part({ tip: B }),
                part({ tip: B }),
            ),
            save: () => {
                if (lost) {
                    lost = false;
                    throw new Error("the answer was lost");
                }
                return { type: "saved", tip: B };
            },
        },
    });
    const files: Record<string, string> = {};
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(files),
        responses: [
            fauxAssistantMessage("Saved."),
            fauxAssistantMessage("Saved again."),
        ],
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    files["memory/a.md"] = "A.";
    await say(agent, "Go.");
    files["memory/b.md"] = "B.";

    await say(agent, "Again.");

    const [first, again, later] = saves(sent);
    assert.deepEqual(again, first);
    assert.deepEqual(
        first?.changes.map(({ path }) => path),
        ["memory/a.md"],
    );
    assert.notEqual(later?.call, first?.call);
    assert.equal(later?.base, B);
    assert.deepEqual(
        later?.changes.map(({ path }) => path),
        ["memory/b.md"],
    );
    assert.equal(saves(sent).length, 3);
    await agent.stop();
});

test("a save the profile answers with an error is sent again under another call, not resent", async (t) => {
    let failed = false;
    const { calls, sent } = fakeCalls({
        profile: {
            ...profileAnswering(part({ tip: A }), part({ tip: B })),
            save: () => {
                if (!failed) {
                    failed = true;
                    throw Object.assign(
                        new Error("save again under a new call"),
                        { group: "core", code: "internal_error" },
                    );
                }
                return { type: "saved", tip: B };
            },
        },
    });
    const files: Record<string, string> = {};
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(files),
        responses: [
            fauxAssistantMessage("Saved."),
            fauxAssistantMessage("Saved again."),
        ],
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    files["memory/a.md"] = "A.";
    await say(agent, "Go.");
    files["memory/b.md"] = "B.";

    await say(agent, "Again.");

    const [first, again] = saves(sent);
    assert.notEqual(again?.call, first?.call);
    assert.deepEqual(
        again?.changes.map(({ path }) => path),
        ["memory/a.md", "memory/b.md"],
    );
    assert.equal(saves(sent).length, 2);
    await agent.stop();
});

test("a pull after a save writes over no file changed while the save was under way, and the next save sends that change", async (t) => {
    const saving = Promise.withResolvers<void>();
    const saved = Promise.withResolvers<{ type: "saved"; tip: string }>();
    const { calls, sent } = fakeCalls({
        profile: {
            ...profileAnswering(
                part({ tip: A, files: [piece("notes.md", "One.")] }),
                part({ tip: B, files: [piece("notes.md", "Theirs.")] }),
                part({ tip: B }),
            ),
            save: () => {
                saving.resolve();
                return saves(sent).length === 1
                    ? saved.promise
                    : { type: "saved", tip: B };
            },
        },
    });
    const files: Record<string, string> = {};
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(files),
        responses: [
            fauxAssistantMessage("Saved."),
            fauxAssistantMessage("Saved again."),
        ],
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    files["memory/a.md"] = "A.";
    await agent.actions.submit(agent.by(from.realm), {
        id: "line-Go.",
        input: { type: "line", text: "Go." },
    });
    await saving.promise;
    files["notes.md"] = "Mine.";
    saved.resolve({ type: "saved", tip: B });
    await agent.idle();

    assert.equal(files["notes.md"], "Mine.");
    await say(agent, "Again.");
    assert.deepEqual(saves(sent)[1]?.changes, [
        { type: "write", ...piece("notes.md", "Mine.") },
    ]);
    await agent.stop();
});

test("a save refused because a purge rewrote its base fills again, keeps what the agent changed but the purge removed, and saves it on the new tip", async (t) => {
    const C = "ef".repeat(20);
    const D = "01".repeat(20);
    const saving = Promise.withResolvers<void>();
    const refused = Promise.withResolvers<SaveAnswer>();
    const { calls, sent } = fakeCalls({
        profile: {
            ...profileAnswering(
                part({
                    tip: A,
                    files: [
                        piece("notes.md", "One."),
                        piece("memory/password.md", "hunter2"),
                    ],
                }),
                part({ tip: C, files: [piece("notes.md", "One.")] }),
                part({ tip: D }),
            ),
            save: () => {
                saving.resolve();
                return saves(sent).length === 1
                    ? refused.promise
                    : { type: "saved", tip: D };
            },
        },
    });
    const files: Record<string, string> = {};
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(files),
        responses: [fauxAssistantMessage("Saved.")],
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    files["notes.md"] = "Mine.";
    files["memory/password.md"] = "hunter3";
    await agent.actions.submit(agent.by(from.realm), {
        id: "line-Go.",
        input: { type: "line", text: "Go." },
    });
    await saving.promise;
    assert.equal(
        await agent.actions.remove_path(agent.by(from.profile(2)), {
            path: "memory/password.md",
        }),
        fixture("profile/remove_path.later.answer"),
    );
    await agent.actions.moved(agent.by(from.profile(2)), { tip: C });
    refused.resolve(
        fixture("profile/save.refused.commit_gone.answer") as SaveAnswer,
    );
    await agent.idle();

    assert.equal(files["notes.md"], "Mine.");
    assert.equal(files["memory/password.md"], undefined);
    const [, again] = saves(sent);
    assert.equal(again?.base, C);
    assert.deepEqual(again?.changes, [
        { type: "write", ...piece("notes.md", "Mine.") },
    ]);
    assert.equal(saves(sent).length, 2);
    await agent.stop();
});

test("a save in two parts whose last answer is lost is sent again from that part, under its call", async (t) => {
    let lost = true;
    const { calls, sent } = fakeCalls({
        profile: {
            ...profileAnswering(part({ tip: A }), part({ tip: B })),
            save: (save) => {
                if (!save.last) return { type: "kept" };
                if (lost) {
                    lost = false;
                    throw new Error("the answer was lost");
                }
                return { type: "saved", tip: B };
            },
        },
    });
    const files: Record<string, string> = {};
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(files),
        responses: [
            fauxAssistantMessage("Saved."),
            fauxAssistantMessage("Saved again."),
        ],
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    files["memory/large.md"] = "x".repeat(800 * 1024);
    await say(agent, "Go.");

    await say(agent, "Again.");

    const sends = saves(sent);
    assert.deepEqual(
        sends.map(({ part }) => part),
        [0, 1, 1],
    );
    assert.equal(new Set(sends.map(({ call }) => call)).size, 1);
    assert.deepEqual(sends[2], sends[1]);
    await agent.stop();
});
