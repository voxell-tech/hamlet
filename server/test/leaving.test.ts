import { randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { erase, merge, record, remove, restore, undo } from "../src/agents.ts";
import { chatOf, submitLine, WithDoc } from "../src/chats.ts";
import { GrantsDoc, give, grantsOf } from "../src/gate.ts";
import { houseOf } from "../src/home.ts";
import { InstructionsDoc } from "../src/instructions.ts";
import { LimitsDoc } from "../src/limits.ts";
import { raise, TrustedDoc, trustKey } from "../src/oversight.ts";
import { resolve, roadsOf } from "../src/realm.ts";
import { agentKey, HOME, RecordDoc } from "../src/record.ts";
import { mayor as seatMayor, VotesDoc } from "../src/votes.ts";
import { mayor, request, served } from "./connection-support.ts";
import {
    addAgent,
    addHamlet,
    context,
    crashing,
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

it("replays a committed removal's result even if a vote opened before its tool result was kept", async () => {
    const { models, faux } = fauxModels();
    const dir = scratch();
    const home = await testHome(dir, {
        models,
        openStorage: crashing((writes) =>
            writes.some(
                (w) =>
                    w.type === "entry" &&
                    w.value.kind === "pi.tool-result" &&
                    JSON.stringify(w.value.model).includes("Removed `scout`"),
            ),
        ),
    });
    await addAgent(home, HOME, "scout");
    await addAgent(home, HOME, "smith");
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("remove_agent", {
                name: "scout",
                reason: "Work ended",
            }),
            { stopReason: "toolUse" },
        ),
    ]);
    const sub = await say(home, "remove scout");
    expect(await home.harness.closed).toMatchObject({ reason: "failed" });
    faux.setResponses([
        (prompt) => {
            expect(JSON.stringify(prompt)).toContain("Removed `scout`");
            return fauxAssistantMessage("done");
        },
    ]);
    const again = await testHome(dir, {
        models,
        parts: [
            (home) => ({
                start: async (c) => {
                    await home.harness.commit(async (tx) => {
                        (await tx.doc(VotesDoc, HOME, HOME)).votes.push({
                            id: 1,
                            motion: { kind: "replace_mayor", candidate: 2 },
                            by: { chief: home.chief },
                            reason: "A vote opened",
                            mayor: 0,
                            voters: [2],
                            answer: null,
                            ballots: [],
                            standing: "open",
                            called: home.now(),
                            task: "test-vote",
                        });
                    }, c);
                },
            }),
        ],
    });
    expect(
        (await (await again.harness.submission(sub.id, context))?.wait(context))
            ?.status,
    ).toBe("done");
    expect(
        (await again.harness.snapshot(RecordDoc, HOME, context))?.archive,
    ).toHaveLength(1);
}, 30_000);

it("stops an archived turn after a crash between departure and stopping it", async () => {
    const { models, faux } = fauxModels();
    let armed = false;
    let departed = false;
    const home = await testHome(scratch(), {
        models,
        openStorage: crashing((writes) => {
            if (!armed) return false;
            if (departed) return true;
            departed = JSON.stringify(writes).includes("Gone at crash");
            return false;
        }),
    });
    const scout = await addAgent(home, HOME, "scout");
    let started = false;
    faux.setResponses([
        (_p, options) =>
            new Promise((_resolve, reject) => {
                started = true;
                options?.signal?.addEventListener(
                    "abort",
                    () => reject(new Error("stopped")),
                    { once: true },
                );
            }),
    ]);
    const sub = await submitLine(
        home,
        { agent: scout, chief: home.chief },
        { id: randomUUID(), text: "Work" },
        context,
    );
    await vi.waitFor(() => expect(started).toBe(true));
    armed = true;
    await expect(
        remove(home, { ...deed, rest: "scout Gone at crash" }, context),
    ).rejects.toThrow();
    await home.harness.closed;
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("write", {
                path: "after.txt",
                content: "must not act after leaving",
            }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("done"),
    ]);
    const again = await testHome(home.dir, { models });
    const resumed = await again.harness.submission(sub.id, context);
    expect((await resumed?.wait(context))?.status).not.toBe("done");
    expect(existsSync(join(houseOf(home.layout, scout), "after.txt"))).toBe(
        false,
    );
}, 30_000);

it.each(["remove", "merge"])(
    "takes conversation trust back when an agent leaves by %s",
    async (how) => {
        const home = await testHome(scratch(), { models: fauxModels().models });
        const scout = await addAgent(home, HOME, "scout");
        await addAgent(home, HOME, "smith");
        const chat = await chatOf(
            home,
            { agent: scout, chief: home.chief },
            context,
        );
        const key = await home.harness.commit(async (tx) => {
            const key = trustKey(await tx.doc(WithDoc, chat.id));
            Object.assign(await tx.doc(TrustedDoc, key, key), {
                edit: true,
                command: true,
            });
            return key;
        }, context);
        if (how === "remove") await remove(home, deed, context);
        else
            await merge(
                home,
                { ...deed, rest: "scout into smith Overlap" },
                context,
            );
        await restore(home, { ...deed, rest: "scout" }, context);
        expect(await home.harness.snapshot(TrustedDoc, key, context)).toEqual({
            edit: false,
            command: false,
        });
    },
);

it.each(["restore", "undo", "split"])(
    "brings the founder back under its own name through %s",
    async (how) => {
        const home = await testHome(scratch(), { models: fauxModels().models });
        await addAgent(home, HOME, "scout");
        await addAgent(home, HOME, "smith");
        await seatMayor(home, { ...deed, rest: "scout New mayor" }, context);
        if (how === "split")
            await merge(
                home,
                { ...deed, rest: "founder into smith Retired" },
                context,
            );
        else await remove(home, { ...deed, rest: "founder Retired" }, context);
        const words =
            how === "undo"
                ? await undo(home, deed, context)
                : await restore(home, { ...deed, rest: "founder" }, context);
        expect(words).toContain("`founder`");
        expect(
            (
                await home.harness.snapshot(RecordDoc, HOME, context)
            )?.agents.find((a) => a.number === 0)?.name,
        ).toBe("founder");
    },
);
