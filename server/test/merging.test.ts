import { randomUUID } from "node:crypto";
import { writeFileSync } from "node:fs";
import {
    mkdir,
    open,
    readFile,
    rename,
    symlink,
    unlink,
    writeFile,
} from "node:fs/promises";
import { dirname, join } from "node:path";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import {
    defineExtension,
    GenerationTask,
    hook,
} from "@earendil-works/pi-durable";
import { expect, it, vi } from "vitest";
import {
    erase as eraseDeed,
    merge as mergeDeed,
    recordLines,
    remove as removeDeed,
    restore as restoreDeed,
} from "../src/agents.ts";
import { InstructionsDoc, submitLine, WithDoc } from "../src/chats.ts";
import { houseOf } from "../src/home/layout.ts";
import { LimitsDoc } from "../src/limits.ts";
import { CharacterDoc } from "../src/prompt.ts";
import { resolveAddress } from "../src/realm.ts";
import { agentKey, carryRoad, HOME, RecordDoc } from "../src/record.ts";
import { overturn as overturnDeed, vote as voteDeed } from "../src/votes.ts";
import { mayor, request, served } from "./connection-support.ts";
import {
    addAgent,
    context,
    deedWords,
    departedAgents,
    fauxAgents,
    fauxModels,
    livingAgents,
    mayorChat,
    request as modelRequest,
    say,
    scratch,
    testHome,
    transcript,
    usedRoad,
    view,
} from "./support.ts";

const deed = {
    hamlet: HOME,
    chief: "chief",
    rest: "scout into librarian They overlap.",
};

async function pair() {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const kept = await addAgent(home, HOME, "librarian", "Keeps the list.");
    const gone = await addAgent(home, HOME, "scout", "Looks things up.");
    const put = async (number: number, rel: string, text: string) => {
        const file = join(houseOf(home.layout, { hamlet: HOME, number }), rel);
        await mkdir(dirname(file), { recursive: true });
        await writeFile(file, text);
    };
    const get = (number: number, rel: string) =>
        readFile(
            join(houseOf(home.layout, { hamlet: HOME, number }), rel),
            "utf8",
        );
    await put(kept.number, "NOTES.md", "# Notes\nAlice likes tea.\n");
    await put(gone.number, "NOTES.md", "# Notes\nThe wiki is slow.\n");
    return { home, faux, kept, gone, put, get, models };
}

it("merges houses, keeps clashes apart, inherits road use and leaves each character alone", async () => {
    const { home, kept, gone, put, get } = await pair();
    await addAgent(home, HOME, "poet");
    await home.harness.commit(async (tx) => {
        for (const [a, b, messages] of [
            ["office", 1, 1],
            ["office", 2, 3],
            [2, 3, 1],
        ] as const)
            await usedRoad(tx, HOME, a, b, { messages, at: 1, since: 1 });
        (await tx.doc(CharacterDoc, agentKey(kept), agentKey(kept))).text =
            "kept";
        (await tx.doc(CharacterDoc, agentKey(gone), agentKey(gone))).text =
            "gone";
    }, context);
    await put(
        1,
        "skills/sort/SKILL.md",
        "---\nname: sort\ndescription: x\n---\nmine",
    );
    await put(
        2,
        "skills/sort/SKILL.md",
        "---\nname: sort\ndescription: x\n---\nRun skills/sort/run.sh",
    );
    await put(
        2,
        "skills/lookup/SKILL.md",
        "---\nname: lookup\ndescription: x\n---\nlookup",
    );
    await put(1, "list.txt", "mine");
    await put(2, "list.txt", "theirs");
    await put(2, "found.txt", "Dune");
    const words = await merge(home, deed, context);
    expect(words).toContain("Merged `scout` into `librarian`");
    expect(words).toContain("its purpose is now: Keeps the list.");
    expect(words).toContain("joined, not rewritten");
    expect((await livingAgents(home)).map((a) => a.name)).toEqual([
        "founder",
        "librarian",
        "poet",
    ]);
    expect(
        (await view(home)).hamlets.find((h) => h.id === HOME)?.roads,
    ).toEqual(
        expect.arrayContaining([
            expect.objectContaining({
                a: { hamlet: HOME, number: 0 },
                b: { hamlet: HOME, number: 1 },
                messages: 4,
            }),
            expect.objectContaining({
                a: { hamlet: HOME, number: 1 },
                b: { hamlet: HOME, number: 3 },
                messages: 1,
            }),
        ]),
    );
    expect(words).toContain("Kept apart because they differed:");
    expect(words).toContain("sort");
    expect(words).toContain("list.txt");
    expect(await get(1, "NOTES.md")).toContain(
        "## From scout\nThe wiki is slow.",
    );
    expect(await get(1, "merged/scout/NOTES.before.md")).toBe(
        "# Notes\nAlice likes tea.\n",
    );
    expect(await get(1, "merged/scout/NOTES.md")).toBe(
        "# Notes\nThe wiki is slow.\n",
    );
    expect(await get(1, "skills/sort-from-scout/SKILL.md")).toContain(
        "name: sort-from-scout",
    );
    expect(await get(1, "skills/sort-from-scout/SKILL.md")).toContain(
        "skills/sort-from-scout/run.sh",
    );
    expect(await get(1, "skills/lookup/SKILL.md")).toContain("lookup");
    expect(await get(1, "list.txt")).toBe("mine");
    expect(await get(1, "merged/scout/list.txt")).toBe("theirs");
    expect(await get(1, "found.txt")).toBe("Dune");
    expect(
        (await home.harness.snapshot(CharacterDoc, agentKey(kept), context))
            ?.text,
    ).toBe("kept");
    expect(
        (await home.harness.snapshot(CharacterDoc, agentKey(gone), context))
            ?.text,
    ).toBe("gone");
    expect(
        await resolveAddress(home.harness, HOME, "scout", context),
    ).toMatchObject({
        ok: false,
        error: {
            kind: "departed",
            farewell: expect.stringContaining("talk to `librarian`"),
        },
    });
}, 60_000);

it("merges by a chief's command over a WebSocket", async () => {
    const run = await served({ models: fauxModels().models });
    await addAgent(run.home, HOME, "librarian", "Keeps the list.");
    await addAgent(run.home, HOME, "scout", "Maps");
    expect(
        await request(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: mayor,
            text: "/merge scout into librarian They overlap",
        }),
    ).toMatchObject({
        type: "reply",
        kind: "notice",
        text: expect.stringContaining("Merged `scout` into `librarian`"),
    });
}, 60_000);

it("rewrites notes in a bare errand and records the new purpose", async () => {
    const { home, faux, get } = await pair();
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, HOME, HOME)).leaving.mergedNotes = 100;
    }, context);
    fauxAgents(faux, {
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("merge_agents", {
                    keep: "librarian",
                    absorb: "scout",
                    purpose: "Both.",
                    reason: "Overlap",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("done"),
        ],
        errands: [
            (prompt, _o, _s, model) => {
                const sent = modelRequest(prompt.messages, model);
                expect(sent.tools).toEqual([]);
                expect(Object.keys(sent.sections)).toEqual(["instructions"]);
                expect(JSON.stringify(prompt)).toContain("Alice likes tea.");
                expect(JSON.stringify(prompt)).toContain("The wiki is slow.");
                return fauxAssistantMessage(
                    "```markdown\n# Notes\nBoth facts.\n```",
                );
            },
        ],
    });
    expect((await (await say(home, "merge")).wait(context)).status).toBe(
        "done",
    );
    expect((await get(1, "NOTES.md")).startsWith("# Notes\nBoth facts.")).toBe(
        true,
    );
    expect(
        (await livingAgents(home)).find((a) => a.ref.number === 1)?.purpose,
    ).toBe("Both.");
    expect((await recordLines(home, HOME, context)).join("\n")).toContain(
        "notes rewritten",
    );
}, 60_000);

it("joins notes without an errand once the budget is spent, and says why", async () => {
    const { home, faux, get } = await pair();
    home.registry.install(
        defineExtension({
            name: "test.spend-before-merge",
            hooks: [
                hook(GenerationTask, {
                    afterResponse: async (_m, api, c) => {
                        const place = await api.snapshot(
                            WithDoc,
                            api.conversationId,
                            c,
                        );
                        if (place?.number === 0)
                            await home.harness.commit(async (tx) => {
                                (
                                    await tx.doc(LimitsDoc, HOME, HOME)
                                ).budget.cost = 0;
                            }, c);
                    },
                }),
            ],
        }),
    );
    fauxAgents(faux, {
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("merge_agents", {
                    keep: "librarian",
                    absorb: "scout",
                    purpose: "Both.",
                    reason: "Overlap",
                }),
                { stopReason: "toolUse" },
            ),
        ],
    });
    const sub = await say(home, "merge");
    await sub.wait(context);
    expect(await get(1, "NOTES.md")).toContain("## From scout");
    expect(faux.state.callCount).toBe(1);
    expect(JSON.stringify(await transcript(await mayorChat(home)))).toContain(
        "spent its budget",
    );
}, 60_000);

it.each([false, true])(
    "splits a merge and preserves learning since: %s",
    async (learned) => {
        const { home, put, get } = await pair();
        await addAgent(home, HOME, "poet");
        await home.harness.commit(async (tx) => {
            await usedRoad(tx, HOME, 2, 3, {
                messages: 1,
                at: 1,
                since: 1,
                by: "mayor",
            });
        }, context);
        await put(2, "found.txt", "Dune");
        await put(2, "old.txt", "old");
        await merge(home, deed, context);
        if (learned) {
            await put(
                1,
                "NOTES.md",
                `${await get(1, "NOTES.md")}Bob likes coffee.\n`,
            );
            await put(1, "found.txt", "Dune, Emma");
            await home.harness.commit(async (tx) => {
                carryRoad(await tx.doc(RecordDoc, HOME, HOME), 1, 3, 1);
            }, context);
        }
        expect(
            await restore(home, { ...deed, rest: "scout" }, context),
        ).toContain("Split `scout` from `librarian`");
        expect(await get(2, "found.txt")).toBe("Dune");
        await expect(get(1, "old.txt")).rejects.toThrow();
        if (learned) {
            expect(await get(1, "found.txt")).toBe("Dune, Emma");
            expect(await get(1, "NOTES.md")).toContain("Bob likes coffee.");
            expect(await get(1, "NOTES.md")).toContain("## Split");
        } else {
            await expect(get(1, "found.txt")).rejects.toThrow();
            expect(await get(1, "NOTES.md")).toBe(
                "# Notes\nAlice likes tea.\n",
            );
            await expect(
                get(1, "merged/scout/NOTES.before.md"),
            ).rejects.toThrow();
        }
    },
    60_000,
);

it("splits merges latest first and requires the kept agent to live", async () => {
    const { home } = await pair();
    await addAgent(home, HOME, "poet", "Verse.");
    await merge(home, deed, context);
    await merge(
        home,
        { ...deed, rest: "poet into librarian More overlap" },
        context,
    );
    await expect(
        restore(home, { ...deed, rest: "scout" }, context),
    ).rejects.toThrow("split that one first");
    await restore(home, { ...deed, rest: "poet" }, context);
    await remove(
        home,
        { ...deed, rest: "librarian No longer needed" },
        context,
    );
    await expect(
        restore(home, { ...deed, rest: "scout" }, context),
    ).rejects.toThrow("restore that first");
}, 60_000);

it("refuses links without reading them, and refuses an oversized house before a merge", async () => {
    const { home, gone, get } = await pair();
    const outside = join(scratch(), "secret");
    await writeFile(outside, "outside secret");
    const link = join(houseOf(home.layout, gone), "leak");
    await symlink(outside, link);
    await expect(merge(home, deed, context)).rejects.toThrow(
        "links or special files",
    );
    expect(await livingAgents(home, HOME)).toHaveLength(3);
    await expect(get(1, "leak")).rejects.toThrow();
    await unlink(link);
    const big = await open(join(houseOf(home.layout, gone), "big"), "w");
    await big.truncate(513 * 1024 * 1024);
    await big.close();
    await expect(merge(home, deed, context)).rejects.toThrow(
        "Remove the agent instead",
    );
    expect(await departedAgents(home, HOME)).toHaveLength(0);
}, 60_000);

it("finishes a merge whose file manifest was left pending, without bringing a file twice", async () => {
    const { home, models, put, get } = await pair();
    await put(2, "found.txt", "Dune");
    await merge(home, deed, context);
    const dir = home.dir;
    await home.harness.commit(async (tx) => {
        const entry = (await tx.doc(RecordDoc, HOME, HOME)).archive[0];
        if (entry?.merge) entry.merge.applied = false;
    }, context);
    await unlink(
        join(houseOf(home.layout, { hamlet: HOME, number: 1 }), "found.txt"),
    );
    await home.close(context);
    const again = await testHome(dir, { models });
    expect(await get(1, "found.txt")).toBe("Dune");
    expect(
        (await again.harness.snapshot(RecordDoc, HOME, context))?.archive[0]
            ?.merge?.applied,
    ).toBe(true);
    await writeFile(
        join(houseOf(home.layout, { hamlet: HOME, number: 1 }), "NOTES.md"),
        "# Notes\nLater.\n",
    );
    await again.close(context);
    await testHome(dir, { models });
    expect(await get(1, "NOTES.md")).toBe("# Notes\nLater.\n");
}, 60_000);

it.each([1, 2])(
    "refuses a mayor's merge while either agent owes a chief a task: %s",
    async (number) => {
        const { home, faux } = await pair();
        await home.harness.commit(async (tx) => {
            (
                await tx.doc(
                    InstructionsDoc,
                    `home:${number}`,
                    `home:${number}`,
                )
            ).given.push({
                task: "held",
                chief: "alice",
                text: "Find Dune.",
                at: home.now(),
                done: null,
                hearing: { agent: false, mayor: false, cleared: false },
            });
        }, context);
        fauxAgents(faux, {
            "home:0": [
                fauxAssistantMessage(
                    fauxToolCall("merge_agents", {
                        keep: "librarian",
                        absorb: "scout",
                        purpose: "Both.",
                        reason: "Overlap",
                    }),
                    { stopReason: "toolUse" },
                ),
                (prompt) => {
                    expect(JSON.stringify(prompt)).toContain("Find Dune.");
                    return fauxAssistantMessage("done");
                },
            ],
        });
        await (await say(home, "merge")).wait(context);
        expect(await departedAgents(home, HOME)).toHaveLength(0);
        expect(faux.state.callCount).toBe(2);
    },
);

it("keeps both agents' notes when two merges into one house are decided together", async () => {
    const { home, put, get } = await pair();
    await addAgent(home, HOME, "poet", "Verse.");
    await put(3, "NOTES.md", "# Notes\nThe sky is blue.\n");
    await Promise.all([
        merge(home, deed, context),
        merge(home, { ...deed, rest: "poet into librarian Overlap" }, context),
    ]);
    expect(await get(1, "NOTES.md")).toContain("The wiki is slow.");
    expect(await get(1, "NOTES.md")).toContain("The sky is blue.");
}, 60_000);

it("refuses a blocked destination before archiving either agent", async () => {
    const { home, put } = await pair();
    await put(1, "blocked", "a file, not a folder");
    await put(2, "blocked/inside", "cannot be delivered here");
    expect(await merge(home, deed, context)).toContain("before merging");
    expect((await livingAgents(home, HOME)).map((a) => a.name)).toEqual([
        "founder",
        "librarian",
        "scout",
    ]);
    expect(await departedAgents(home, HOME)).toHaveLength(0);
}, 30_000);

it("opens with a blocked pending merge and retries it through merge without splitting", async () => {
    const { home, models, put, get } = await pair();
    await put(2, "found.txt", "Dune");
    await merge(home, deed, context);
    await home.harness.commit(async (tx) => {
        const a = (await tx.doc(RecordDoc, HOME, HOME)).archive[0];
        if (a?.merge) a.merge.applied = false;
    }, context);
    await put(1, "found.txt", "Keep this instead");
    await home.close(context);
    const again = await testHome(home.dir, { models });
    expect(
        (await again.harness.snapshot(RecordDoc, HOME, context))?.archive[0]
            ?.merge?.applied,
    ).toBe(false);
    expect(JSON.stringify(await transcript(await mayorChat(again)))).toContain(
        "/merge",
    );
    await unlink(
        join(houseOf(again.layout, { hamlet: HOME, number: 1 }), "found.txt"),
    );
    expect(await merge(again, deed, context)).toContain("Merged");
    expect(await get(1, "found.txt")).toBe("Dune");
    expect(
        (await again.harness.snapshot(RecordDoc, HOME, context))?.archive[0]
            ?.back,
    ).toBeNull();
    expect(await get(2, "found.txt")).toBe("Dune");
}, 60_000);

it("stops the absorbed turn before reading its house and keeps both agents stopped during the notes errand", async () => {
    const { home, faux, gone, get, put } = await pair();
    await put(2, "found.txt", "Before stopping");
    let started = false;
    let stopped = false;
    let stoppedDuringErrand = false;
    let restartedStatus = "";
    fauxAgents(faux, {
        "home:2": [
            (_prompt, options) =>
                new Promise((_resolve, reject) => {
                    started = true;
                    options?.signal?.addEventListener(
                        "abort",
                        () => {
                            writeFileSync(
                                join(houseOf(home.layout, gone), "found.txt"),
                                "Last work before stopping",
                            );
                            stopped = true;
                            reject(new Error("stopped"));
                        },
                        { once: true },
                    );
                }),
        ],
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("merge_agents", {
                    keep: "librarian",
                    absorb: "scout",
                    purpose: "Both",
                    reason: "Overlap",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("done"),
        ],
        errands: [
            async () => {
                stoppedDuringErrand = stopped;
                if (!stopped)
                    return fauxAssistantMessage("# Notes\nBoth facts.");
                const restarted = await submitLine(
                    home,
                    { agent: gone, chief: "chief" },
                    { id: randomUUID(), text: "Work again" },
                    context,
                );
                restartedStatus = (await restarted.wait(context)).status;
                return fauxAssistantMessage("# Notes\nBoth facts.");
            },
        ],
    });
    const busy = await submitLine(
        home,
        { agent: gone, chief: "chief" },
        { id: randomUUID(), text: "Work" },
        context,
    );
    await vi.waitFor(() => expect(started).toBe(true));
    await home.harness.commit(async (tx) => {
        (await tx.doc(InstructionsDoc, agentKey(gone), agentKey(gone))).given =
            [];
    }, context);
    await (await say(home, "merge")).wait(context);
    expect((await busy.wait(context)).status).not.toBe("done");
    expect(stoppedDuringErrand).toBe(true);
    expect(restartedStatus).not.toBe("");
    expect(restartedStatus).not.toBe("done");
    expect(await get(1, "found.txt")).toBe("Last work before stopping");
}, 60_000);

it("keeps newer notes intact when a pending merge resumes", async () => {
    const { home, models, put, get } = await pair();
    await merge(home, deed, context);
    await home.harness.commit(async (tx) => {
        const a = (await tx.doc(RecordDoc, HOME, HOME)).archive[0];
        if (a?.merge) a.merge.applied = false;
    }, context);
    await put(1, "NOTES.md", "# Notes\nLearned later: Bob likes coffee.\n");
    await home.close(context);
    const again = await testHome(home.dir, { models });
    expect(await get(1, "NOTES.md")).toContain("Bob likes coffee.");
    expect(
        (await again.harness.snapshot(RecordDoc, HOME, context))?.archive[0]
            ?.merge?.applied,
    ).toBe(false);
}, 60_000);

it("opens with a blocked pending split and retries its files through restore", async () => {
    const { home, models, get } = await pair();
    await merge(home, deed, context);
    await restore(home, { ...deed, rest: "scout" }, context);
    await home.harness.commit(async (tx) => {
        const a = (await tx.doc(RecordDoc, HOME, HOME)).archive[0];
        if (a?.merge) a.merge.applied = false;
    }, context);
    const notes = join(
        houseOf(home.layout, { hamlet: HOME, number: 1 }),
        "NOTES.md",
    );
    await unlink(notes);
    const outside = join(scratch(), "secret");
    await writeFile(outside, "outside");
    await symlink(outside, notes);
    await home.close(context);
    const again = await testHome(home.dir, { models });
    expect(
        (await again.harness.snapshot(RecordDoc, HOME, context))?.archive[0]
            ?.merge?.applied,
    ).toBe(false);
    await unlink(notes);
    await writeFile(notes, "# Notes\nAlice likes tea.\n");
    expect(await restore(again, { ...deed, rest: "scout" }, context)).toContain(
        "is finished",
    );
    expect(await get(1, "NOTES.md")).toContain("Alice likes tea.");
}, 60_000);

it("refuses to erase an unapplied merge and keeps its archived house available for retry", async () => {
    const { home, get } = await pair();
    await merge(home, deed, context);
    await home.harness.commit(async (tx) => {
        const a = (await tx.doc(RecordDoc, HOME, HOME)).archive[0];
        if (a?.merge) a.merge.applied = false;
    }, context);
    expect(await erase(home, { ...deed, rest: "scout" }, context)).toContain(
        "merge has not finished",
    );
    const archive = (await home.harness.snapshot(RecordDoc, HOME, context))
        ?.archive[0]?.id;
    const refusal = await erase(home, { ...deed, rest: "scout" }, context);
    expect(refusal).toContain(`/merge ${archive} into librarian`);
    expect(refusal).not.toContain(`/restore ${archive}`);
    expect(await departedAgents(home, HOME)).toHaveLength(1);
    expect(await get(2, "NOTES.md")).toContain("The wiki is slow.");
}, 60_000);

it.each(["folder-link", "dangling-link"])(
    "refuses a %s destination before committing the merge",
    async (kind) => {
        const { home, put } = await pair();
        const root = houseOf(home.layout, { hamlet: HOME, number: 1 });
        if (kind === "folder-link") {
            await mkdir(join(root, "sub"));
            await symlink("sub", join(root, "docs"));
            await put(2, "docs/a.txt", "must stay safe");
        } else {
            await symlink("missing", join(root, "a.txt"));
            await put(2, "a.txt", "must stay safe");
        }
        let words: string;
        try {
            words = await merge(home, deed, context);
        } catch (error) {
            if (!(error instanceof Error)) throw error;
            words = error.message;
        }
        expect(await departedAgents(home, HOME)).toHaveLength(0);
        expect(words).toContain("before merging");
    },
    60_000,
);

it("keeps the absorbed notes' original bytes even when they are not UTF-8", async () => {
    const { home, gone } = await pair();
    const bytes = Buffer.from([0xff, 0xfe]);
    await writeFile(join(houseOf(home.layout, gone), "NOTES.md"), bytes);
    await merge(home, deed, context);
    expect(
        (await home.harness.snapshot(RecordDoc, HOME, context))?.archive[0]
            ?.merge?.applied,
    ).toBe(true);
    expect(
        await readFile(
            join(
                houseOf(home.layout, { hamlet: HOME, number: 1 }),
                "merged/scout/NOTES.md",
            ),
        ),
    ).toEqual(bytes);
}, 60_000);

it("delivers a whole planned tree with bounded sandbox launches, including existing parent folders", async () => {
    const { home, put, get } = await pair();
    await put(1, "docs/mine.txt", "mine");
    for (let i = 0; i < 40; i++)
        await put(2, `docs/file-${i}.txt`, `file ${i}`);
    const commands = vi.spyOn(home.sandbox, "command");
    try {
        await merge(home, deed, context);
        expect(commands.mock.calls.length).toBeLessThan(20);
        expect(await get(1, "docs/file-39.txt")).toBe("file 39");
        expect(await get(1, "docs/mine.txt")).toBe("mine");
    } finally {
        commands.mockRestore();
    }
}, 60_000);

it("does not absorb a voter when a vote opens during the mayor's notes errand", async () => {
    const { home, faux } = await pair();
    await addAgent(home, HOME, "poet");
    fauxAgents(faux, {
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("merge_agents", {
                    keep: "librarian",
                    absorb: "scout",
                    purpose: "Both",
                    reason: "Overlap",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("done"),
        ],
        errands: [
            async () => {
                await vote(
                    home,
                    { ...deed, rest: "scout New direction" },
                    context,
                );
                return fauxAssistantMessage("# Notes\nBoth facts.");
            },
            (_p, options) =>
                new Promise((_resolve, reject) => {
                    options?.signal?.addEventListener(
                        "abort",
                        () => reject(new Error("stopped")),
                        { once: true },
                    );
                }),
        ],
    });
    await (await say(home, "merge")).wait(context);
    expect(await departedAgents(home, HOME)).toHaveLength(0);
    expect(JSON.stringify(await transcript(await mayorChat(home)))).toContain(
        "nothing else is decided",
    );
    await overturn(home, { ...deed, rest: "1 Stop now" }, context);
}, 60_000);

it("refuses unreadable kept notes before committing and preserves every original byte", async () => {
    const { home, kept } = await pair();
    const file = join(houseOf(home.layout, kept), "NOTES.md");
    const bytes = Buffer.concat([
        Buffer.from("# Notes\nAlice likes tea.\nI had a caf"),
        Buffer.from([0xe9]),
        Buffer.from(" today.\n"),
    ]);
    await writeFile(file, bytes);
    const words = await merge(home, deed, context);
    expect(await departedAgents(home, HOME)).toHaveLength(0);
    expect(await readFile(file)).toEqual(bytes);
    expect(words).toContain("UTF-8");
}, 60_000);

it("finishes pending merge files with /merge while running, without splitting or archiving twice", async () => {
    const { home, put, get } = await pair();
    await put(2, "found.txt", "Dune");
    await merge(home, deed, context);
    await home.harness.commit(async (tx) => {
        const entry = (await tx.doc(RecordDoc, HOME, HOME)).archive[0];
        if (!entry?.merge) throw new Error("No merge");
        entry.merge.applied = false;
    }, context);
    await put(1, "found.txt", "Keep this instead");
    await expect(merge(home, deed, context)).resolves.toContain(
        "files are still pending",
    );
    await unlink(
        join(houseOf(home.layout, { hamlet: HOME, number: 1 }), "found.txt"),
    );
    const archived = (await home.harness.snapshot(RecordDoc, HOME, context))
        ?.archive[0];
    if (!archived) throw new Error("No archive");
    const retry = { ...deed, rest: `${archived.id} into librarian` };
    expect(await merge(home, retry, context)).toContain(
        "Merged `scout` into `librarian`",
    );
    expect(await merge(home, retry, context)).toContain(
        "Merged `scout` into `librarian`",
    );
    const record = await home.harness.snapshot(RecordDoc, HOME, context);
    expect(record?.agents.map((a) => a.name)).toEqual(["founder", "librarian"]);
    expect(record?.archive).toHaveLength(1);
    expect(record?.archive[0]).toMatchObject({
        back: null,
        merge: { applied: true },
    });
    expect(await get(1, "found.txt")).toBe("Dune");
    expect(await restore(home, { ...deed, rest: "scout" }, context)).toContain(
        "Split",
    );
    expect((await livingAgents(home, HOME)).map((a) => a.name)).toContain(
        "scout",
    );
});

it("merges a new living agent when an older archive has the same absorbed name", async () => {
    const { home, put, get } = await pair();
    await merge(home, deed, context);
    const keptHouse = houseOf(home.layout, { hamlet: HOME, number: 1 });
    await rename(
        join(keptHouse, "merged/scout"),
        join(keptHouse, "merged/old-scout"),
    );
    const scout = await addAgent(home, HOME, "scout");
    await put(scout.number, "new.txt", "New work");
    expect(await merge(home, deed, context)).toContain(
        "Merged `scout` into `librarian`",
    );
    expect(await departedAgents(home, HOME)).toHaveLength(2);
    expect(await get(1, "new.txt")).toBe("New work");
});

const erase = deedWords(eraseDeed);
const merge = deedWords(mergeDeed);
const remove = deedWords(removeDeed);
const restore = deedWords(restoreDeed);

const vote = deedWords(voteDeed);
const overturn = deedWords(overturnDeed);

it("records applied once the merge's files have been delivered", async () => {
    const { home, put, get } = await pair();
    await put(2, "found.txt", "Dune");
    expect(await merge(home, deed, context)).toContain(
        "Merged `scout` into `librarian`",
    );
    expect(await get(1, "found.txt")).toBe("Dune");
    expect(
        (await home.harness.snapshot(RecordDoc, HOME, context))?.archive[0]
            ?.merge?.applied,
    ).toBe(true);
});
