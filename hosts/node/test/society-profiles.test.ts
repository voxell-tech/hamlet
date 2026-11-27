import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import {
    cappedFiles,
    fakeCalls,
    fakeFiles,
    from,
    line,
    named,
    resultTexts,
    shown,
    timedOut,
    toolCall,
    wakeAgent,
} from "./agent-testing.ts";
import { acts, COMMIT, seatAndHire, seated } from "./society-testing.ts";
import { context } from "./testing.ts";

test("a seated mayor is named with its hamlet, in its words", async (t) => {
    const { results } = await seatAndHire(t, ["hire#22"]);
    assert.deepEqual(results, [`${shown(22).name} is the mayor of h now.`]);
});

test("an address is trimmed", async (t) => {
    const { seat } = await seatAndHire(t, [
        " hire#4 ",
        " mayor ",
        "mayor@ h1 ",
    ]);
    assert.deepEqual(seat, [
        seated(4),
        { hire: { type: "mayor" } },
        { hire: { type: "mayor_of", hamlet: "h1" } },
    ]);
});

test("an address's number is read as an i64, where the act can carry it", async (t) => {
    const read = [
        "hire#007",
        "hire#+3",
        "hire#0",
        "hire#-0",
        "hire#4294967295",
        "worker#3",
    ];
    const refused = [
        "mayor@",
        "hire#",
        "hire#-1",
        "hire#4294967296",
        "hire#0x10",
        "hire#1e3",
        "worker#",
        "helper#3",
    ];
    const { seat, hire, results } = await seatAndHire(
        t,
        [...read, ...refused],
        ["profile#007", "profile#0", "profile#+3", "profile#-3"],
    );

    assert.deepEqual(seat, [
        seated(7),
        seated(3),
        seated(0),
        seated(0),
        seated(4294967295),
        { hire: { type: "worker", worker: 3 } },
    ]);
    assert.deepEqual(hire, [
        { purpose: "Dig", profile: 7 },
        { purpose: "Dig", profile: 0 },
        { purpose: "Dig", profile: 3 },
    ]);
    assert.deepEqual(
        results.slice(read.length, read.length + refused.length),
        refused.map((address) => `\`${address}\` is not an address`),
    );
    assert.equal(results.at(-1), "`profile#-3` is not an address");
});

test("a reply the realm refuses, or one that fails, is reported and leaves the run and the next turn as they were", async (t) => {
    let replies = 0;
    const { calls, sent } = fakeCalls({
        realm: {
            reply: () => {
                replies += 1;
                if (replies > 1) throw timedOut();
                return {
                    type: "refused",
                    why: { type: "no_question" },
                    words: "the question is answered",
                };
            },
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            fauxAssistantMessage("Two of them."),
            fauxAssistantMessage("Three of them."),
            fauxAssistantMessage("Done."),
        ],
    });
    const question = (id: string, letter: number) => ({
        id,
        input: {
            type: "question" as const,
            letter,
            from: { agent: 12, name: "Ada" },
            text: "How many fail?",
        },
    });
    await agent.actions.submit(agent.by(from.realm), question("input-1", 5));
    await agent.idle();
    await agent.actions.submit(agent.by(from.realm), question("input-2", 6));
    await agent.idle();
    await agent.actions.submit(agent.by(from.realm), line("input-3", "On."));
    await agent.idle();
    const answers = (await agent.entries())
        .filter((entry) => entry.kind === "pi.assistant")
        .map((entry) => entry.text);
    const harness = await agent.harness();
    const settled = await harness.commit(async (tx) => {
        const conversations = (await tx.scanConversations({}, 100)).items;
        return Promise.all(
            ["input-1", "input-2", "input-3"].map(async (id) => {
                for (const { id: conversation } of conversations) {
                    const found = await tx.submissionByRequest(
                        conversation,
                        id,
                    );
                    if (found !== undefined) return found.status;
                }
                return undefined;
            }),
        );
    }, context);
    await agent.stop();

    assert.deepEqual(
        named(sent, "reply").map((call) => call.message),
        [
            { question: 5, text: "Two of them." },
            { question: 6, text: "Three of them." },
        ],
    );
    // The questions are answered in their peer's chat, the line in the chief's.
    assert.deepEqual(answers, ["Done."]);
    assert.deepEqual(settled, ["done", "done", "done"]);
    assert.deepEqual(agent.reported, [
        "a reply was refused: the question is answered",
        "a reply was not taken: actor.action_timed_out: The action timed out.",
    ]);
});

test("end_hire's reason is trimmed, and a blank one is left out", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: { end: () => ({ type: "done", agent: shown(3) }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("end_hire", { hire: "hire#4", reason: " Done. " }, "c1"),
            toolCall("end_hire", { hire: "hire#4", reason: "  " }, "c2"),
            fauxAssistantMessage("Ended."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "End."));
    await agent.idle();
    await agent.stop();

    const four = { type: "agent", agent: 4 };
    assert.deepEqual(acts(sent, "end"), [
        { hire: four, why: "Done." },
        { hire: four },
    ]);
});

test("the profile tools' arguments leave as their acts: `profile#<n>` as its profile, `main`, a commit or a version as a revision, and an empty path as a listing", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            create_profile: () => ({
                type: "made",
                profile: 4,
                label: "profile#4 (Ada)",
            }),
            read_profile: () => ({ type: "listed", paths: [] }),
            edit_profile: () => ({ type: "done" }),
            tag: () => ({ type: "tagged", version: "v1", commit: COMMIT }),
            revert: () => ({ type: "unchanged" }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("create_profile", { name: " Ada ", character: "" }, "c1"),
            toolCall(
                "create_profile",
                { name: "Ada", character: "You write careful tests." },
                "c2",
            ),
            toolCall(
                "read_profile",
                { profile: " profile#4 ", path: "" },
                "c3",
            ),
            toolCall(
                "read_profile",
                {
                    profile: "profile#4",
                    path: "memory/tests.md",
                    version: "v1",
                },
                "c4",
            ),
            toolCall(
                "read_profile",
                { profile: "profile#4", version: "main" },
                "c5",
            ),
            toolCall(
                "read_profile",
                { profile: "profile#4", version: COMMIT },
                "c6",
            ),
            toolCall(
                "edit_profile",
                {
                    profile: "profile#4",
                    path: " CHARACTER.md ",
                    content: "You write careful tests.",
                    message: "Sharper",
                },
                "c7",
            ),
            toolCall(
                "tag",
                {
                    profile: "profile#4",
                    tag: " v1 ",
                    note: "Before the refactor",
                },
                "c8",
            ),
            toolCall(
                "revert",
                {
                    profile: "profile#4",
                    commit: COMMIT,
                    reason: "It broke the tests",
                },
                "c9",
            ),
            toolCall("revert", { profile: "profile#4", commit: "v1" }, "c10"),
            toolCall("read_profile", { profile: "Ada" }, "c11"),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Go."));
    await agent.idle();

    assert.deepEqual(
        {
            create_profile: acts(sent, "create_profile"),
            read_profile: acts(sent, "read_profile"),
            edit_profile: acts(sent, "edit_profile"),
            tag: acts(sent, "tag"),
            revert: acts(sent, "revert"),
        },
        {
            create_profile: [
                { name: "Ada" },
                { name: "Ada", character: "You write careful tests." },
            ],
            read_profile: [
                { profile: 4 },
                {
                    profile: 4,
                    at: { type: "version", version: "v1" },
                    path: "memory/tests.md",
                },
                { profile: 4, at: { type: "main" } },
                { profile: 4, at: { type: "commit", commit: COMMIT } },
            ],
            edit_profile: [
                {
                    profile: 4,
                    path: "CHARACTER.md",
                    text: "You write careful tests.",
                    message: "Sharper",
                },
            ],
            tag: [{ profile: 4, version: "v1", note: "Before the refactor" }],
            revert: [
                {
                    profile: 4,
                    what: { type: "commit", commit: COMMIT },
                    reason: "It broke the tests",
                },
                { profile: 4, what: { type: "version", version: "v1" } },
            ],
        },
    );
    assert.equal((await resultTexts(agent)).at(-1), "`Ada` is not an address");
    await agent.stop();
});

test("the profile tools answer in their words", async (t) => {
    const answers = {
        read_profile: [
            {
                type: "listed",
                paths: [{ path: "memory/a.md" }, { path: "skills/b/SKILL.md" }],
            },
            { type: "listed", paths: [] },
            { type: "text", text: "# Tests\n", cut: false },
            { type: "text", text: "# Tests", cut: true },
        ],
    } as const;
    const { calls } = fakeCalls({
        realm: {
            create_profile: () => ({
                type: "made",
                profile: 4,
                label: "profile#4 (Ada)",
            }),
            read_profile: () => answers.read_profile[read++] as never,
            ask_librarian: () => ({ type: "done" }),
        },
    });
    let read = 0;
    const addressed = { profile: " profile#4 " };
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("create_profile", { name: "Ada" }, "c1"),
            ...[2, 3, 4, 5].map((n) =>
                toolCall(
                    "read_profile",
                    { ...addressed, path: n > 3 ? "a" : "" },
                    `c${n}`,
                ),
            ),
            toolCall(
                "ask_librarian",
                { question: "How do I lint the docs?" },
                "c10",
            ),
            toolCall("ask_librarian", { question: "  " }, "c11"),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Go."));
    await agent.idle();

    assert.deepEqual(await resultTexts(agent), [
        "The profile profile#4 (Ada) lives in the city; hire it next.",
        "- memory/a.md\n- skills/b/SKILL.md",
        "profile#4 has no files.",
        "# Tests\n",
        "# Tests\n[cut here]",
        "The librarian is looking.",
        "Say what you need a skill for.",
    ]);
    await agent.stop();
});

test("submit_skill offers its folder's files to the library under the tool's call, then names the kept offer to the realm; a name, a missing folder or the library's refusal is refused in its words", async (t) => {
    const { calls, sent } = fakeCalls({
        library: {
            offer: (offer) =>
                offer.skill === "taken"
                    ? {
                          type: "refused",
                          why: { type: "already_shelved", skill: "taken" },
                          words: "a skill named `taken` is on the shelves already",
                      }
                    : { type: "kept", offer: 9 },
        },
        realm: { submit_skill: () => ({ type: "done" }) },
    });
    const files = fakeFiles({
        "skills/fixtures/SKILL.md": "---\nname: fixtures\n---\n",
        "skills/fixtures/bin/run.sh": "echo run\n",
        "skills/taken/SKILL.md": "---\nname: taken\n---\n",
        "memory/a.md": "Not a skill.",
    });
    const agent = await wakeAgent(t, {
        calls,
        files,
        responses: [
            toolCall(
                "submit_skill",
                { skill: " fixtures ", reason: "Every hire writes fixtures" },
                "c1",
            ),
            toolCall("submit_skill", { skill: "../fixtures" }, "c2"),
            toolCall("submit_skill", { skill: "missing" }, "c3"),
            toolCall("submit_skill", { skill: "taken" }, "c4"),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Share it.", "task-1"),
    );
    await agent.idle();

    const offers = named(sent, "offer").map((call) => call.message);
    assert.deepEqual(offers[0], {
        call: (offers[0] as { call: string }).call,
        skill: "fixtures",
        reason: "Every hire writes fixtures",
        part: 0,
        last: true,
        files: [
            {
                path: "SKILL.md",
                offset: 0,
                bytes: Buffer.from("---\nname: fixtures\n---\n").toString(
                    "base64",
                ),
                last: true,
            },
            {
                path: "bin/run.sh",
                offset: 0,
                bytes: Buffer.from("echo run\n").toString("base64"),
                last: true,
            },
        ],
    });
    assert.match((offers[0] as { call: string }).call, /^t[1-9][0-9]*$/);
    assert.deepEqual(acts(sent, "submit_skill"), [
        { offer: 9, skill: "fixtures", reason: "Every hire writes fixtures" },
    ]);
    assert.deepEqual(await resultTexts(agent), [
        "The librarian is judging it.",
        "`skill` is its folder under skills/, in letters, digits, hyphens and underscores.",
        "no skill is named `missing`",
        "a skill named `taken` is on the shelves already",
    ]);
    await agent.stop();
});

test("submit_skill of a folder whose walk stops at its cap is refused, and nothing is offered", async (t) => {
    const { calls, sent } = fakeCalls({
        library: { offer: () => ({ type: "kept", offer: 9 }) },
        realm: { submit_skill: () => ({ type: "done" }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        files: cappedFiles({
            "skills/fixtures/SKILL.md": "---\nname: fixtures\n---\n",
        }),
        responses: [
            toolCall("submit_skill", { skill: "fixtures" }, "c1"),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Share it."),
    );
    await agent.idle();

    assert.deepEqual(named(sent, "offer"), []);
    assert.deepEqual(await resultTexts(agent), [
        "`fixtures` holds more files than the agent can list, 20000, so it was not offered.",
    ]);
    await agent.stop();
});
