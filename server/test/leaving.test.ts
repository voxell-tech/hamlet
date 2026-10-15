import { randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { submitLine } from "../src/chats.ts";
import { GrantsDoc, give, grantsOf } from "../src/grants.ts";
import { agentKey, HOME } from "../src/hamlet.ts";
import { InstructionsDoc } from "../src/instructions.ts";
import { houseOf } from "../src/layout.ts";
import { LimitsDoc } from "../src/limits.ts";
import { resolve, roadsOf } from "../src/realm.ts";
import { RecordDoc } from "../src/record.ts";
import {
    erase,
    record,
    remove,
    restore,
    undo,
} from "../src/society/leaving.ts";
import { raise } from "../src/waits.ts";
import { mayor, request, served } from "./connection-support.ts";
import {
    addAgent,
    addHamlet,
    context,
    fauxModels,
    openWaits,
    say,
    scratch,
    testHome,
} from "./support.ts";

const deed = { hamlet: HOME, chief: "chief", rest: "scout Work ended" };

it("archives an agent with its roads and reason, keeps its house and refuses its address", async () => {
    const home = await testHome(scratch(), {
        models: fauxModels().models,
        now: () => 1234,
    });
    const scout = await addAgent(home, HOME, "scout", "Looks things up.");
    await addAgent(home, HOME, "smith");
    await writeFile(join(houseOf(home.layout, scout), "found.txt"), "Dune");
    await home.harness.commit(async (tx) => {
        (await tx.doc(RecordDoc, HOME, HOME)).roads.push({
            a: 1,
            b: 2,
            messages: 3,
            lastFrom: 1,
            lastUsed: 100,
            since: 1,
            by: "mayor",
        });
        await give(tx, scout, "scout", "network", "chief", 1);
    }, context);
    expect(await remove(home, deed, context)).toBe(
        "Removed `scout`. Its house is archived as scout-1234 and its roads are gone. A chief can restore it.",
    );
    const stored = await home.harness.snapshot(RecordDoc, HOME, context);
    expect(stored?.archive).toMatchObject([
        {
            id: "scout-1234",
            reason: "Work ended",
            agent: { number: 1, purpose: "Looks things up." },
            roads: expect.arrayContaining([
                expect.objectContaining({ a: 1, b: 2, messages: 3 }),
            ]),
        },
    ]);
    expect(stored && roadsOf(stored).some((r) => r.a === 1 || r.b === 1)).toBe(
        false,
    );
    expect(
        await readFile(join(houseOf(home.layout, scout), "found.txt"), "utf8"),
    ).toBe("Dune");
    expect(await resolve(home.harness, HOME, "scout", context)).toMatchObject({
        gone: expect.stringContaining(
            'was removed from the hamlet by the chief chief ("Work ended")',
        ),
    });
    expect(grantsOf(await home.harness.snapshot(GrantsDoc, context))).toEqual(
        [],
    );
    expect(await record(home, deed, context)).toContain(
        "the chief chief removed `scout`: Work ended",
    );
    expect(await record(home, deed, context)).toContain("/restore scout-1234");
});

it("refuses the mayor, an unknown agent, a blank reason and an agent already gone; concurrent removal wins once", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    await addAgent(home, HOME, "scout");
    for (const [rest, words] of [
        ["mayor x", "mayor cannot"],
        ["nobody x", "no agent"],
        ["scout  ", "reason"],
    ])
        await expect(
            remove(home, { ...deed, rest: rest ?? "" }, context),
        ).rejects.toThrow(words);
    const results = await Promise.allSettled([
        remove(home, deed, context),
        remove(home, deed, context),
    ]);
    expect(results.filter((r) => r.status === "fulfilled")).toHaveLength(1);
    expect(results.find((r) => r.status === "rejected")).toMatchObject({
        reason: expect.objectContaining({
            message: expect.stringContaining("was removed"),
        }),
    });
});

it("protects a chief's task from the mayor, including the interval after its answer, but not from a chief", async () => {
    const { models, faux } = fauxModels();
    let now = 100;
    const home = await testHome(scratch(), { models, now: () => now });
    const scout = await addAgent(home, HOME, "scout");
    await home.harness.commit(async (tx) => {
        (
            await tx.doc(InstructionsDoc, agentKey(scout), agentKey(scout))
        ).given.push({
            task: "task",
            chief: "alice",
            text: "Find Dune.",
            at: 1,
            done: null,
            hearing: { agent: false, mayor: false, cleared: false },
        });
        (await tx.doc(LimitsDoc, HOME, HOME)).leaving.protectedMinutes = 1;
    }, context);
    const tryMayor = async (words: string) => {
        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("remove_agent", {
                    name: "scout",
                    reason: "Not needed.",
                }),
                { stopReason: "toolUse" },
            ),
            (prompt) => {
                expect(JSON.stringify(prompt)).toContain(words);
                return fauxAssistantMessage("done");
            },
        ]);
        expect(
            (await (await say(home, "remove it")).wait(context)).status,
        ).toBe("done");
    };
    await tryMayor("is not finished");
    await home.harness.commit(async (tx) => {
        const i = (
            await tx.doc(InstructionsDoc, agentKey(scout), agentKey(scout))
        ).given[0];
        if (i) i.done = now;
    }, context);
    await tryMayor("has only just ended");
    now += 60_000;
    await tryMayor("Removed `scout`");
    await restore(home, { ...deed, rest: "scout" }, context);
    await home.harness.commit(async (tx) => {
        const i = (
            await tx.doc(InstructionsDoc, agentKey(scout), agentKey(scout))
        ).given[0];
        if (i) i.done = null;
    }, context);
    expect(await remove(home, deed, context)).toContain("Removed `scout`");
});

it("stops a departing agent in its turn and lapses its waits", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, HOME, "scout");
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("bash", { command: "true", network: true }),
            { stopReason: "toolUse" },
        ),
    ]);
    const sub = await submitLine(
        home,
        { agent: scout, chief: home.chief },
        { id: randomUUID(), text: "work" },
        context,
    );
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(1));
    await home.harness.commit(
        (tx) =>
            raise(
                tx,
                {
                    kind: "fault",
                    danger: null,
                    by: "scout",
                    agent: scout,
                    about: "old fault",
                    answers: ["dismiss"],
                    change: null,
                    chat: null,
                    call: null,
                },
                home.now(),
            ),
        context,
    );
    await remove(home, deed, context);
    expect((await sub.wait(context)).status).not.toBe("done");
    expect(await openWaits(home)).toHaveLength(0);
});

it("restores only roads to the agents it knew, free and even over the road limit; a taken name needs as", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    await addAgent(home, HOME, "scout", "Maps");
    await addAgent(home, HOME, "smith");
    await addAgent(home, HOME, "poet");
    await home.harness.commit(async (tx) => {
        const r = await tx.doc(RecordDoc, HOME, HOME);
        for (const b of [2, 3])
            r.roads.push({
                a: 1,
                b,
                messages: 4,
                lastFrom: 1,
                lastUsed: 1,
                since: 1,
                by: "mayor",
            });
        (await tx.doc(LimitsDoc, HOME, HOME)).mostRoads = 0;
    }, context);
    await remove(home, deed, context);
    await remove(home, { ...deed, rest: "poet x" }, context);
    await addAgent(home, HOME, "poet");
    await addHamlet(home, "mill", "miller");
    await addAgent(home, "mill", "scout");
    await expect(
        restore(home, { ...deed, rest: "scout" }, context),
    ).rejects.toThrow("as <new-name>");
    const words = await restore(
        home,
        { ...deed, rest: "scout as old-scout It was a mistake" },
        context,
    );
    expect(words).toContain("Restored `old-scout`");
    expect(words).toContain("more than the 0 your limits allow");
    const r = await home.harness.snapshot(RecordDoc, HOME, context);
    expect(r?.agents.find((a) => a.name === "old-scout")).toMatchObject({
        number: 1,
        purpose: "Maps",
    });
    expect(
        r?.roads.filter(
            (r) =>
                (r.a === 1 || r.b === 1) &&
                r.a !== "office" &&
                r.b !== "office",
        ),
    ).toEqual([expect.objectContaining({ a: 1, b: 2, messages: 4 })]);
});

it("refuses a full hamlet, and erases an archived house and entry for good", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const scout = await addAgent(home, HOME, "scout");
    await expect(
        erase(home, { ...deed, rest: "scout" }, context),
    ).rejects.toThrow("/remove it first");
    await remove(home, deed, context);
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, HOME, HOME)).mostAgents = 0;
    }, context);
    await expect(
        restore(home, { ...deed, rest: "scout" }, context),
    ).rejects.toThrow("full");
    expect(await erase(home, { ...deed, rest: "scout" }, context)).toContain(
        "for good",
    );
    expect(existsSync(houseOf(home.layout, scout))).toBe(false);
    expect(
        (await home.harness.snapshot(RecordDoc, HOME, context))?.archive,
    ).toEqual([]);
    await expect(
        restore(home, { ...deed, rest: "scout" }, context),
    ).rejects.toThrow("nothing is archived");
});

it("runs a chief's remove, restore, record and undo over a WebSocket", async () => {
    const run = await served({ models: fauxModels().models });
    await addAgent(run.home, HOME, "scout");
    const command = (text: string) =>
        request(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: mayor,
            text,
        });
    expect(await command("/remove scout Work ended")).toMatchObject({
        type: "reply",
        kind: "notice",
        text: expect.stringContaining("Removed `scout`"),
    });
    expect(await command("/record")).toMatchObject({
        text: expect.stringContaining("/restore scout-"),
    });
    expect(await command("/restore scout")).toMatchObject({
        text: expect.stringContaining("Restored `scout`"),
    });
    await command("/remove scout Work ended");
    expect(await command("/undo")).toMatchObject({
        text: expect.stringContaining("Restored `scout`"),
    });
    expect(await undo(run.home, deed, context)).toContain("Nothing to undo");
});
