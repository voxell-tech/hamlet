import { randomUUID } from "node:crypto";
import { access } from "node:fs/promises";
import { join } from "node:path";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { WorkDoc } from "../src/hamlet.ts";
import { LimitsDoc } from "../src/limits.ts";
import { RealmDoc } from "../src/realm.ts";
import { RecordDoc } from "../src/record.ts";
import { request as send, served } from "./connection-support.ts";
import {
    addAgent,
    addHamlet,
    chatWith,
    context,
    fauxModels,
    openWaits,
    request,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

it("founds a chief's hamlet, reaches its mayor, and gives the first mayor the realm tools", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    await chatWith(run.home, { hamlet: "home", number: 0 });
    const deed = (text: string) =>
        send(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: { hamlet: "home", agent: "mayor" },
            text,
        });
    expect(await deed("/found mill miller Grinds flour.")).toMatchObject({
        type: "reply",
        kind: "notice",
        text: "The hamlet `mill` is founded under the mayor `miller`, and it is yours. /hamlet mill talks to it.",
    });
    expect(await run.home.harness.snapshot(RealmDoc, context)).toMatchObject({
        hamlets: [
            { id: "home" },
            { id: "mill", chiefs: ["chief"], removed: null },
        ],
    });
    expect(
        await run.home.harness.snapshot(RecordDoc, "mill", context),
    ).toMatchObject({
        mayor: 0,
        next: 1,
        agents: [{ number: 0, name: "miller", purpose: "Grinds flour." }],
    });
    expect(
        await run.home.harness.snapshot(WorkDoc, "mill", context),
    ).toBeDefined();
    expect(
        await run.home.harness.snapshot(LimitsDoc, "mill", context),
    ).toBeDefined();
    await access(join(run.home.layout.houses, "mill/0/NOTES.md"));
    faux.setResponses([fauxAssistantMessage("Fresh flour.")]);
    expect(
        await send(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: { hamlet: "mill", agent: "mayor" },
            text: "Hello",
        }),
    ).toMatchObject({ type: "reply", kind: "notice" });
    faux.setResponses([
        (prompt, _o, _s, model) => {
            const seen = request(prompt.messages, model);
            expect(seen.tools).toContain("ask_removal");
            expect(seen.sections.realm).toContain("mill: mayor `miller`");
            return fauxAssistantMessage("done");
        },
    ]);
    expect(await deed("Hello home")).toMatchObject({
        type: "reply",
        kind: "notice",
    });
    expect(await deed("/hamlets")).toMatchObject({
        text: expect.stringContaining("mill"),
    });
});

it("refuses taken names, empty purposes, used ids and the home's hamlet cap", async () => {
    const run = await served({ models: fauxModels().models });
    const deed = (text: string) =>
        send(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: { hamlet: "home", agent: "mayor" },
            text,
        });
    for (const [text, words] of [
        ["/found mill miller", "purpose"],
        ["/found mill mayor Work", "reserved"],
        ["/found mill founder Work", "reserved"],
    ])
        expect(await deed(text ?? "")).toMatchObject({
            kind: "error",
            text: expect.stringContaining(words ?? ""),
        });
    await deed("/found mill miller Flour");
    expect(await deed("/found dock miller Ships")).toMatchObject({
        kind: "error",
        text: expect.stringContaining("already exists"),
    });
    await run.home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).hamlets.most = 2;
    }, context);
    expect(await deed("/found dock docker Ships")).toMatchObject({
        kind: "error",
        text: expect.stringContaining("no more hamlets"),
    });
    await deed("/remove-hamlet mill Done");
    expect(await deed("/found mill another Flour")).toMatchObject({
        kind: "error",
        text: expect.stringContaining("used before"),
    });
});

it("names, places, removes, restores and erases a hamlet without reusing its id", async () => {
    const run = await served({ models: fauxModels().models });
    const deed = (text: string) =>
        send(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: { hamlet: "home", agent: "mayor" },
            text,
        });
    await deed("/found mill miller Flour");
    expect(await deed(`/name-hamlet mill ${"x".repeat(33)}`)).toMatchObject({
        kind: "error",
        text: expect.stringContaining("32 letters"),
    });
    expect(await deed("/name-hamlet mill The   Mill")).toMatchObject({
        kind: "notice",
    });
    expect(await deed("/move-hamlet mill -12.5 40")).toMatchObject({
        kind: "notice",
    });
    expect(
        (await run.home.harness.snapshot(RealmDoc, context))?.hamlets[1],
    ).toMatchObject({ title: "The Mill", at: { x: -12.5, z: 40 } });
    expect(await deed("/move-hamlet mill Infinity 0")).toMatchObject({
        kind: "error",
    });
    expect(await deed("/name-hamlet mill")).toMatchObject({ kind: "notice" });
    expect(
        (await run.home.harness.snapshot(RealmDoc, context))?.hamlets[1]?.title,
    ).toBeNull();
    expect(await deed("/remove-hamlet home")).toMatchObject({
        kind: "error",
        text: expect.stringContaining("cannot be removed"),
    });
    expect(await deed("/erase-hamlet mill")).toMatchObject({ kind: "error" });
    expect(await deed("/remove-hamlet mill Done")).toMatchObject({
        kind: "notice",
    });
    await access(join(run.home.layout.houses, "mill/0/NOTES.md"));
    expect(await deed("/restore-hamlet mill")).toMatchObject({
        kind: "notice",
    });
    await deed("/remove-hamlet mill Done again");
    expect(await deed("/erase-hamlet mill")).toMatchObject({ kind: "notice" });
    await expect(
        access(join(run.home.layout.houses, "mill")),
    ).rejects.toThrow();
    expect(await deed("/restore-hamlet mill")).toMatchObject({ kind: "error" });
});

it("records removal asks without removing and counts only distinct live mayors since restore", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const refs = {
        mill: await addHamlet(home, "mill", "miller"),
        dock: await addHamlet(home, "dock", "docker"),
        farm: await addHamlet(home, "farm", "farmer"),
        fort: await addHamlet(home, "fort", "keeper"),
    };
    await home.harness.commit(async (tx) => {
        const mill = (await tx.doc(RealmDoc)).hamlets.find(
            (h) => h.id === "mill",
        );
        if (!mill) throw new Error("No mill");
        mill.chiefs = [];
    }, context);
    const ask = async (by: keyof typeof refs, target = "mill") => {
        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("ask_removal", { hamlet: target, reason: "Idle" }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("done"),
        ]);
        const chat = await chatWith(home, refs[by]);
        expect(
            (
                await (
                    await chat.submit(
                        { type: "input", content: "Ask" },
                        context,
                    )
                ).wait(context)
            ).status,
        ).toBe("done");
        return JSON.stringify(await transcript(chat));
    };
    expect(await ask("dock")).toContain("Not enough mayors");
    expect(await ask("farm")).toContain("Not enough mayors");
    expect(await ask("farm")).toContain("Not enough mayors");
    expect(await ask("fort")).toContain("/remove-hamlet mill confirms it");
    expect(
        JSON.stringify(await transcript(await chatWith(home, refs.mill))),
    ).toContain("/remove-hamlet mill confirms it");

    expect(
        (await home.harness.snapshot(RealmDoc, context))?.hamlets.find(
            (h) => h.id === "mill",
        )?.removed,
    ).toBeNull();
    const { say } = await import("../src/say.ts");
    const deed = (text: string) =>
        say(
            home,
            {
                type: "say",
                id: randomUUID(),
                to: { hamlet: "home", agent: "mayor" },
                text,
            },
            context,
        );
    await deed("/remove-hamlet dock Done");
    expect(await ask("farm")).toContain("Not enough mayors");
    expect(await ask("mill")).toContain("/remove-hamlet mill confirms it");
    await deed("/remove-hamlet mill Done");
    await deed("/restore-hamlet mill");
    expect(
        (await home.harness.snapshot(RealmDoc, context))?.removals.filter(
            (r) => r.hamlet === "mill",
        ),
    ).toEqual([]);
}, 30000);

it("stops removed work, lapses waits and refuses restore if a name was taken", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    const deed = (text: string) =>
        send(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: { hamlet: "home", agent: "mayor" },
            text,
        });
    await deed("/found mill miller Flour");
    const miller = { hamlet: "mill", number: 0 };
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("bash", { command: "echo hello", network: true }),
            { stopReason: "toolUse" },
        ),
    ]);
    const { submitLine } = await import("../src/chats.ts");
    const submission = await submitLine(
        run.home,
        { agent: miller, chief: run.home.chief },
        { id: randomUUID(), text: "Use network" },
        context,
    );
    await vi.waitFor(
        async () => expect(await openWaits(run.home)).toHaveLength(1),
        { timeout: 10000 },
    );
    await deed("/remove-hamlet mill Done");
    expect((await submission.wait(context)).status).toBe("unanswered");
    expect(await openWaits(run.home)).toHaveLength(0);
    await addAgent(run.home, "home", "miller");
    expect(await deed("/restore-hamlet mill")).toMatchObject({
        kind: "error",
        text: expect.stringContaining("exists now"),
    });
});

it("shows an agent its cross-hamlet road and refuses another chief's changes", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    const scout = await addAgent(run.home, "home", "scout");
    await addHamlet(run.home, "mill", "miller");
    const smith = await addAgent(run.home, "mill", "smith");
    await run.home.harness.commit(async (tx) => {
        const realm = await tx.doc(RealmDoc);
        realm.roads.push({
            a: scout,
            b: smith,
            agreed: ["home", "mill"],
            reason: "Trade",
            messages: 0,
            lastFrom: null,
            lastUsed: 0,
        });
        const mill = realm.hamlets.find((h) => h.id === "mill");
        if (mill) mill.chiefs = ["another"];
    }, context);
    let words: string | undefined;
    faux.setResponses([
        (prompt, _o, _s, model) => {
            words = request(prompt.messages, model).sections.realm;
            return fauxAssistantMessage("done");
        },
    ]);
    const chat = await chatWith(run.home, scout);
    await (await chat.submit({ type: "input", content: "Work" }, context)).wait(
        context,
    );
    expect(words).toBe(
        "Your roads to agents of other hamlets lead to: smith. Whoever asks pays for the answer.",
    );
    for (const text of [
        "/remove-hamlet mill",
        "/name-hamlet mill Mine",
        "/move-hamlet mill 0 0",
    ])
        expect(
            await send(run.connection, run.messages, {
                type: "say",
                id: randomUUID(),
                to: { hamlet: "home", agent: "mayor" },
                text,
            }),
        ).toMatchObject({
            kind: "error",
            text: expect.stringContaining("only a chief"),
        });
});

it("keeps removal, restore and reopening working after erasing another hamlet", async () => {
    const { submitLine } = await import("../src/chats.ts");
    const { models, faux } = fauxModels();
    const run = await served({ models });
    const deed = (text: string) =>
        send(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: { hamlet: "home", agent: "mayor" },
            text,
        });
    await deed("/found mill miller Flour");
    await deed("/found dock docker Boats");
    expect(await deed("/remove-hamlet mill Done")).toMatchObject({
        kind: "notice",
    });
    expect(await deed("/erase-hamlet mill")).toMatchObject({ kind: "notice" });
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("bash", { command: "echo hello", network: true }),
            { stopReason: "toolUse" },
        ),
    ]);
    const submission = await submitLine(
        run.home,
        { agent: { hamlet: "dock", number: 0 }, chief: run.home.chief },
        { id: randomUUID(), text: "Use network" },
        context,
    );
    await vi.waitFor(
        async () => expect(await openWaits(run.home)).toHaveLength(1),
        { timeout: 10000 },
    );
    expect(await deed("/remove-hamlet dock Done")).toMatchObject({
        kind: "notice",
    });
    expect((await submission.wait(context)).status).toBe("unanswered");
    expect(await openWaits(run.home)).toHaveLength(0);
    expect(await deed("/restore-hamlet dock")).toMatchObject({
        kind: "notice",
    });
    await run.connection.close();
    await run.server.close();
    await run.home.close(context);
    const again = await testHome(run.home.dir, { models });
    await access(join(again.layout.houses, "dock/0/NOTES.md"));
    await expect(access(join(again.layout.houses, "mill"))).rejects.toThrow();
});

it("logs a refused cleanup and still processes other hamlets and keeps watching the realm", async () => {
    const { mkdir, readFile, rm, symlink, writeFile } = await import(
        "node:fs/promises"
    );
    const { models } = fauxModels();
    const run = await served({ models });
    const deed = (text: string) =>
        send(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: { hamlet: "home", agent: "mayor" },
            text,
        });
    await deed("/found mill miller Flour");
    await deed("/found dock docker Boats");
    await deed("/remove-hamlet mill Done");
    const outside = scratch();
    await writeFile(join(outside, "canary"), "untouched");
    const mill = join(run.home.layout.houses, "mill");
    await rm(mill, { recursive: true });
    await symlink(outside, mill);
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
        expect(await deed("/erase-hamlet mill")).toMatchObject({
            kind: "notice",
        });
        expect(await deed("/remove-hamlet dock Done")).toMatchObject({
            kind: "notice",
        });
        expect(await deed("/restore-hamlet dock")).toMatchObject({
            kind: "notice",
        });
        expect(logged).toHaveBeenCalledWith(expect.stringContaining("mill"));
        expect(await readFile(join(outside, "canary"), "utf8")).toBe(
            "untouched",
        );
        const dock = join(run.home.layout.houses, "dock/0");
        await rm(dock, { recursive: true });
        await run.home.harness.commit(async (tx) => {
            const place = (await tx.doc(RealmDoc)).hamlets.find(
                (h) => h.id === "dock",
            );
            if (!place) throw new Error("no dock");
            place.title = "Dock";
        }, context);
        await vi.waitFor(
            async () => {
                await access(join(dock, "NOTES.md"));
            },
            { timeout: 10000 },
        );
    } finally {
        logged.mockRestore();
        await rm(mill);
        await mkdir(mill);
    }
});

it.each(["mill", "dock"])(
    "refuses removing or erasing pending move endpoint %s without losing the house",
    async (endpoint) => {
        const { readFile, writeFile } = await import("node:fs/promises");
        const { houseOf } = await import("../src/home.ts");
        const run = await served({ models: fauxModels().models });
        await addHamlet(run.home, "mill", "miller");
        await addHamlet(run.home, "dock", "docker");
        const smith = await addAgent(run.home, "mill", "smith");
        const notes = join(houseOf(run.home.layout, smith), "NOTES.md");
        await writeFile(notes, "precious");
        await run.home.harness.commit(async (tx) => {
            const old = await tx.doc(RecordDoc, "mill", "mill");
            const target = await tx.doc(RecordDoc, "dock", "dock");
            const agent = old.agents.find((a) => a.number === smith.number);
            if (!agent) throw new Error("no smith");
            const next = { hamlet: "dock", number: target.next++ };
            target.agents.push({ ...agent, number: next.number });
            old.agents = old.agents.filter((a) => a.number !== smith.number);
            (await tx.doc(RealmDoc)).pending = { from: smith, to: next };
        }, context);
        const deed = (text: string) =>
            send(run.connection, run.messages, {
                type: "say",
                id: randomUUID(),
                to: { hamlet: "home", agent: "mayor" },
                text,
            });
        expect(await deed(`/remove-hamlet ${endpoint} Done`)).toMatchObject({
            kind: "error",
            text: expect.stringContaining("house move"),
        });
        expect(
            (await run.home.harness.snapshot(RealmDoc, context))?.hamlets.find(
                (h) => h.id === endpoint,
            )?.removed,
        ).toBeNull();
        // A removed endpoint can also be left by an interrupted earlier cleanup.
        await run.home.harness.commit(async (tx) => {
            const place = (await tx.doc(RealmDoc)).hamlets.find(
                (h) => h.id === endpoint,
            );
            if (!place) throw new Error("no endpoint");
            place.removed = {
                at: run.home.now(),
                by: run.home.chief,
                reason: "Done",
            };
        }, context);
        expect(await deed(`/erase-hamlet ${endpoint}`)).toMatchObject({
            kind: "error",
            text: expect.stringContaining("house move"),
        });
        expect(await readFile(notes, "utf8")).toBe("precious");
        expect(
            (await run.home.harness.snapshot(RealmDoc, context))?.hamlets.find(
                (h) => h.id === endpoint,
            )?.erased,
        ).toBe(false);
    },
);
