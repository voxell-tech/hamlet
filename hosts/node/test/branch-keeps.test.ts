import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import { cappedFiles, fakeCalls, from, wakeAgent } from "./agent-testing.ts";
import { A, part, profileAnswering, saves, say } from "./branch-testing.ts";

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
