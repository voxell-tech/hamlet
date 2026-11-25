import assert from "node:assert/strict";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { FilesAnswer } from "../src/agent/messages/profile.ts";
import {
    fakeCalls,
    fakeFiles,
    from,
    held,
    named,
    wakeAgent,
} from "./agent-testing.ts";
import { A, B, part, saves } from "./branch-testing.ts";
import { scratchFolder } from "./testing.ts";

const C = "ef".repeat(20);

/** A profile whose `files` and `save` answer each with the next of theirs. */
function profile(files: FilesAnswer[], tips: string[]) {
    return {
        files: () => {
            const next = files.shift();
            assert.ok(next, "the profile was asked for more files");
            return next;
        },
        save: () => {
            const tip = tips.shift();
            assert.ok(tip, "the profile was asked for more saves");
            return { type: "saved" as const, tip };
        },
    };
}

test("a retired agent saves its unsaved files, and a repeat saves what changed since", async (t) => {
    const { calls, sent } = fakeCalls({
        profile: profile(
            [part({ tip: A }), part({ tip: B }), part({ tip: C })],
            [B, C],
        ),
    });
    const files: Record<string, string> = {};
    const agent = await wakeAgent(t, { calls, files: fakeFiles(files) });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    const realm = agent.by(from.realm);

    files["memory/notes.md"] = "# Notes\n";
    assert.deepEqual(await agent.actions.retire(realm, null), { tip: B });
    files["memory/more.md"] = "More.\n";
    assert.deepEqual(await agent.actions.retire(realm, null), { tip: C });
    await agent.stop();

    assert.deepEqual(
        saves(sent).map(({ base, changes }) => [
            base,
            changes.map(({ path }) => path),
        ]),
        [
            [A, ["memory/notes.md"]],
            [B, ["memory/more.md"]],
        ],
    );
    assert.deepEqual(agent.reported, []);
});

test("a retired agent stops its turn", async (t) => {
    const { calls, sent } = fakeCalls({
        profile: profile([part({ tip: A })], []),
    });
    const slow = held();
    const agent = await wakeAgent(t, {
        calls,
        responses: [slow.step, fauxAssistantMessage("After.")],
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    await agent.actions.submit(agent.by(from.realm), {
        id: "line-1",
        input: { type: "line", text: "Take your time" },
    });
    await slow.asked;

    assert.deepEqual(await agent.actions.retire(agent.by(from.realm), null), {
        tip: A,
    });
    await agent.idle();
    slow.answer(fauxAssistantMessage("Slow."));
    const said = (await agent.entries()).map((entry) => entry.text);
    await agent.stop();

    assert.ok(!said.includes("Slow."), said.join("\n"));
    assert.equal(agent.faux.state.callCount, 1);
    assert.deepEqual(
        named(sent, "cancel").map((call) => call.message),
        [{ which: { type: "all" } }],
    );
});

test("an erased agent's files are gone, and it asks its host to destroy it", async (t) => {
    const root = join(scratchFolder(t), "7.fs");
    const { calls, sent } = fakeCalls({
        profile: profile([part({ tip: A })], []),
    });
    const agent = await wakeAgent(t, {
        calls,
        files: { ...fakeFiles({}), root },
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    mkdirSync(join(root, "memory"), { recursive: true });
    writeFileSync(join(root, "memory", "notes.md"), "Gone soon.\n");

    assert.equal(await agent.actions.erase(agent.by(from.realm), null), null);
    await agent.stop();

    assert.ok(!existsSync(root), `${root} is still there`);
    assert.equal(agent.destroyed, 1);
    assert.deepEqual(saves(sent), []);
});

test("an erase asked again answers again and asks its host to destroy it again", async (t) => {
    const root = join(scratchFolder(t), "7.fs");
    const { calls } = fakeCalls({ profile: profile([part({ tip: A })], []) });
    const agent = await wakeAgent(t, {
        calls,
        files: { ...fakeFiles({}), root },
    });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });

    for (const _ of [1, 2]) {
        assert.equal(
            await agent.actions.erase(agent.by(from.realm), null),
            null,
        );
    }
    await agent.stop();

    assert.ok(!existsSync(root), `${root} is still there`);
    assert.equal(agent.destroyed, 2);
});
