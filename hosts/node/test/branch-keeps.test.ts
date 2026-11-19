import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
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
