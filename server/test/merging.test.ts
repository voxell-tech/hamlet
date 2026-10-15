import { randomUUID } from "node:crypto";
import {
    mkdir,
    open,
    readFile,
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
import { expect, it } from "vitest";
import { CharacterDoc } from "../src/character.ts";
import { WithDoc } from "../src/chats.ts";
import { agentKey, HOME } from "../src/hamlet.ts";
import { InstructionsDoc } from "../src/instructions.ts";
import { houseOf } from "../src/layout.ts";
import { LimitsDoc } from "../src/limits.ts";
import { resolve, roadsOf } from "../src/realm.ts";
import { RecordDoc } from "../src/record.ts";
import { remove, restore } from "../src/society/leaving.ts";
import { merge } from "../src/society/merging.ts";
import { mayor, request, served } from "./connection-support.ts";
import {
    addAgent,
    context,
    fauxAgents,
    fauxModels,
    mayorChat,
    request as modelRequest,
    say,
    scratch,
    testHome,
    transcript,
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
        const r = await tx.doc(RecordDoc, HOME, HOME);
        for (const [a, b, messages] of [
            ["office", 1, 1],
            ["office", 2, 3],
            [2, 3, 1],
        ] as const)
            r.roads.push({
                a,
                b,
                messages,
                lastFrom: a,
                lastUsed: 1,
                since: 1,
                by: "mayor",
            });
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
    expect(words).toContain("joined, not rewritten");
    const r = await home.harness.snapshot(RecordDoc, HOME, context);
    expect(r?.agents.map((a) => a.name)).toEqual([
        "founder",
        "librarian",
        "poet",
    ]);
    expect(r && roadsOf(r)).toEqual(
        expect.arrayContaining([
            expect.objectContaining({ a: "office", b: 1, messages: 4 }),
            expect.objectContaining({ a: 1, b: 3, messages: 1 }),
        ]),
    );
    expect(r?.archive[0]?.merge).toMatchObject({
        applied: true,
        clashes: expect.arrayContaining([
            expect.stringContaining("sort"),
            expect.stringContaining("list.txt"),
        ]),
    });
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
    expect(await resolve(home.harness, HOME, "scout", context)).toMatchObject({
        gone: expect.stringContaining("talk to `librarian`"),
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

it.each([
    ["```markdown\n# Notes\nBoth facts.\n```", true],
    ["<think>hm</think>\nHere:\n# Notes\nBoth facts.", true],
    ["# Notes\n", false],
    ["x".repeat(101), false],
])(
    "accepts usable rewritten notes only: %s",
    async (reply, accepted) => {
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
                    expect(Object.keys(sent.sections)).toEqual([
                        "instructions",
                    ]);
                    expect(JSON.stringify(prompt)).toContain(
                        "Alice likes tea.",
                    );
                    expect(JSON.stringify(prompt)).toContain(
                        "The wiki is slow.",
                    );
                    return fauxAssistantMessage(reply);
                },
            ],
        });
        expect((await (await say(home, "merge")).wait(context)).status).toBe(
            "done",
        );
        expect(
            (await get(1, "NOTES.md")).startsWith("# Notes\nBoth facts."),
        ).toBe(accepted);
        const r = await home.harness.snapshot(RecordDoc, HOME, context);
        expect(r?.agents.find((a) => a.number === 1)?.purpose).toBe("Both.");
        expect(r?.decisions.findLast((d) => d.kind === "merged")).toMatchObject(
            { notes: accepted ? "rewritten" : "joined" },
        );
    },
    60_000,
);

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
            (await tx.doc(RecordDoc, HOME, HOME)).roads.push({
                a: 2,
                b: 3,
                messages: 1,
                lastFrom: 2,
                lastUsed: 1,
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
                const road = (await tx.doc(RecordDoc, HOME, HOME)).roads.find(
                    (r) => r.a === 1 && r.b === 3,
                );
                if (road) road.messages++;
            }, context);
        }
        expect(
            await restore(home, { ...deed, rest: "scout" }, context),
        ).toContain("Split `scout` from `librarian`");
        const r = await home.harness.snapshot(RecordDoc, HOME, context);
        expect(r?.agents.find((a) => a.number === 1)?.purpose).toBe(
            "Keeps the list.",
        );
        expect(r?.agents.find((a) => a.number === 2)?.purpose).toBe(
            "Looks things up.",
        );
        expect(await get(2, "found.txt")).toBe("Dune");
        await expect(get(1, "old.txt")).rejects.toThrow();
        if (learned) {
            expect(await get(1, "found.txt")).toBe("Dune, Emma");
            expect(await get(1, "NOTES.md")).toContain("Bob likes coffee.");
            expect(await get(1, "NOTES.md")).toContain("## Split");
            expect(r?.roads).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({ a: 1, b: 3, messages: 1 }),
                ]),
            );
        } else {
            await expect(get(1, "found.txt")).rejects.toThrow();
            expect(await get(1, "NOTES.md")).toBe(
                "# Notes\nAlice likes tea.\n",
            );
            expect(r?.roads.some((r) => r.a === 1 && r.b === 3)).toBe(false);
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
    expect(
        (await home.harness.snapshot(RecordDoc, HOME, context))?.agents,
    ).toHaveLength(3);
    await expect(get(1, "leak")).rejects.toThrow();
    await unlink(link);
    const big = await open(join(houseOf(home.layout, gone), "big"), "w");
    await big.truncate(513 * 1024 * 1024);
    await big.close();
    await expect(merge(home, deed, context)).rejects.toThrow(
        "Remove the agent instead",
    );
    expect(
        (await home.harness.snapshot(RecordDoc, HOME, context))?.archive,
    ).toHaveLength(0);
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
        expect(
            (await home.harness.snapshot(RecordDoc, HOME, context))?.archive,
        ).toHaveLength(0);
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
