import { randomUUID } from "node:crypto";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it } from "vitest";
import { submitLine } from "../src/chats.ts";
import type { AgentRef } from "../src/hamlet.ts";
import type { Home } from "../src/home.ts";
import { RealmDoc, realmRoads } from "../src/realm.ts";
import { RecordDoc } from "../src/record.ts";
import {
    addAgent,
    addHamlet,
    chatWith,
    context,
    type Faux,
    fauxModels,
    MAYOR,
    request,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

async function call(
    home: Home,
    faux: Faux,
    mayor: AgentRef,
    tool: string,
    args: Parameters<typeof fauxToolCall>[1],
) {
    faux.setResponses([
        fauxAssistantMessage(fauxToolCall(tool, args), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("done"),
    ]);
    const chat = await chatWith(home, mayor);
    const sub = await submitLine(
        home,
        { agent: mayor, chief: home.chief },
        { id: randomUUID(), text: "Decide" },
        context,
    );
    expect((await sub.wait(context)).status).toBe("done");
    return JSON.stringify(
        (await transcript(chat)).findLast((e) => e.kind === "pi.tool-result"),
    );
}

it("opens a cross-hamlet road only when both mayors agree and tells the other mayor what awaits", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const mill = await addHamlet(home, "mill", "miller");
    const scout = await addAgent(home, "home", "scout");
    const smith = await addAgent(home, "mill", "smith");
    const roads = async () => {
        const realm = await home.harness.snapshot(RealmDoc, context);
        const first = await home.harness.snapshot(RecordDoc, "home", context);
        const mill = await home.harness.snapshot(RecordDoc, "mill", context);
        if (!realm || !first || !mill) throw new Error("no realm");
        return realmRoads(realm, { home: first, mill });
    };
    await call(home, faux, MAYOR, "agree_road", {
        a: "scout",
        b: "smith",
        reason: "Trade",
    });
    expect(
        (await roads()).some(
            (r) => r.a.number === scout.number && r.b.number === smith.number,
        ),
    ).toBe(false);
    let words: string | undefined;
    faux.setResponses([
        (prompt, _o, _s, model) => {
            words = request(prompt.messages, model).sections.realm;
            return fauxAssistantMessage("done");
        },
    ]);
    const chat = await chatWith(home, mill);
    await (await chat.submit({ type: "input", content: "Look" }, context)).wait(
        context,
    );
    expect(words).toContain(
        "agreed to a road between `scout` and `smith`: Trade",
    );
    expect(
        await call(home, faux, mill, "agree_road", {
            a: "smith",
            b: "scout",
            reason: "Trade too",
        }),
    ).toContain("A road now joins");
    expect(
        (await roads()).some(
            (r) =>
                r.a.hamlet === "home" &&
                r.a.number === scout.number &&
                r.b.hamlet === "mill" &&
                r.b.number === smith.number,
        ),
    ).toBe(true);
});

it("moves the house and conversations to a new number and keeps only roads both mayors named", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const mill = await addHamlet(home, "mill", "miller");
    await addAgent(home, "mill", "smith");
    const scout = await addAgent(home, "home", "scout");
    const baker = await addAgent(home, "home", "baker");
    const clerk = await addAgent(home, "home", "clerk");
    const { writeFile, readFile, access } = await import("node:fs/promises");
    const { houseOf } = await import("../src/layout.ts");
    const { CharacterDoc } = await import("../src/character.ts");
    const { agentKey } = await import("../src/hamlet.ts");
    const { give, GrantsDoc, grantsOf } = await import("../src/grants.ts");
    const chat = await chatWith(home, scout);
    await writeFile(
        `${houseOf(home.layout, scout)}/NOTES.md`,
        "Remember the mill",
    );
    await home.harness.commit(async (tx) => {
        (await tx.doc(CharacterDoc, agentKey(scout), agentKey(scout))).text =
            "Curious";
        await give(tx, scout, "scout", "network", home.chief, home.now());
        for (const b of [baker.number, clerk.number])
            (await tx.doc(RecordDoc, "home", "home")).roads.push({
                a: scout.number,
                b,
                messages: 4,
                lastFrom: scout.number,
                lastUsed: home.now(),
                since: 1,
                by: "mayor",
            });
    }, context);
    await call(home, faux, MAYOR, "agree_move", {
        agent: "scout",
        to: "mill",
        keep_roads: ["baker", "clerk"],
        reason: "Learn",
    });
    let words: string | undefined;
    faux.setResponses([
        (prompt, _o, _s, model) => {
            words = request(prompt.messages, model).sections.realm;
            return fauxAssistantMessage("done");
        },
    ]);
    await (
        await (
            await chatWith(home, mill)
        ).submit({ type: "input", content: "Look" }, context)
    ).wait(context);
    expect(words).toContain(
        "`scout` moves to `mill`, keeping its roads to [baker, clerk]",
    );
    expect(
        await call(home, faux, mill, "agree_move", {
            agent: "scout",
            to: "mill",
            keep_roads: ["baker"],
            reason: "Welcome",
        }),
    ).toContain("moved from");
    const moved = { hamlet: "mill", number: 2 };
    expect(
        (await home.harness.snapshot(RecordDoc, "home", context))?.agents.some(
            (a) => a.name === "scout",
        ),
    ).toBe(false);
    expect(
        (await home.harness.snapshot(RecordDoc, "mill", context))?.agents.find(
            (a) => a.name === "scout",
        ),
    ).toMatchObject({ number: 2, purpose: "" });
    expect(
        await readFile(`${houseOf(home.layout, moved)}/NOTES.md`, "utf8"),
    ).toBe("Remember the mill");
    await expect(access(houseOf(home.layout, scout))).rejects.toThrow();
    expect(
        await home.harness.snapshot(CharacterDoc, agentKey(moved), context),
    ).toMatchObject({ text: "Curious" });
    expect(grantsOf(await home.harness.snapshot(GrantsDoc, context))).toEqual(
        [],
    );
    expect((await chatWith(home, moved)).id).toBe(chat.id);
    const realm = await home.harness.snapshot(RealmDoc, context);
    expect(realm?.pending).toBeNull();
    expect(realm?.moves).toEqual([]);
    expect(realm?.roads).toMatchObject([
        { a: moved, b: baker, agreed: ["home", "mill"] },
    ]);
    expect(
        (await home.harness.snapshot(RecordDoc, "home", context))?.roads,
    ).toEqual([]);
    faux.setResponses([
        (prompt, _o, _s, model) => {
            const seen = request(prompt.messages, model);
            expect(seen.sections.machine).toContain("/houses/mill/2");
            expect(seen.sections.character).toContain("Curious");
            return fauxAssistantMessage("home now");
        },
    ]);
    expect(
        (
            await (
                await submitLine(
                    home,
                    { agent: moved, chief: home.chief },
                    { id: randomUUID(), text: "Continue" },
                    context,
                )
            ).wait(context)
        ).status,
    ).toBe("done");
});

it("refuses roads and moves outside the two mayors' authority or the chief's limits", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const mill = await addHamlet(home, "mill", "miller");
    const dock = await addHamlet(home, "dock", "docker");
    await addAgent(home, "home", "scout");
    await addAgent(home, "home", "baker");
    await addAgent(home, "mill", "smith");
    const road = (by: AgentRef, a: string, b: string) =>
        call(home, faux, by, "agree_road", { a, b, reason: "Trade" });
    const move = (by: AgentRef, agent: string) =>
        call(home, faux, by, "agree_move", {
            agent,
            to: "mill",
            reason: "Learn",
        });
    expect(await road(MAYOR, "scout", "baker")).toContain("same hamlet");
    expect(await road(MAYOR, "mayor", "smith")).toContain(
        "mayors already have roads",
    );
    expect(await road(dock, "scout", "smith")).toContain("neither");
    await road(MAYOR, "scout", "smith");
    expect(await road(MAYOR, "scout", "smith")).toContain("agreed already");
    await road(mill, "smith", "scout");
    expect(await road(MAYOR, "scout", "smith")).toContain(
        "already have a road",
    );
    const { LimitsDoc } = await import("../src/limits.ts");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).hamlets.roadsBetween = false;
    }, context);
    expect(await road(MAYOR, "baker", "smith")).toContain("allow no roads");
    expect(await move(MAYOR, "founder")).toContain("mayor never leaves");
    expect(await move(dock, "scout")).toContain("only the mayor");
    expect(await move(mill, "smith")).toContain("already lives");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "mill", "mill")).mostAgents = 2;
    }, context);
    expect(await move(MAYOR, "scout")).toContain("is full");
}, 30000);

it("finishes a committed house move on reopening without losing the chief's chat", async () => {
    const { mkdir, writeFile, rm, readFile } = await import("node:fs/promises");
    const { houseOf } = await import("../src/layout.ts");
    const { models, faux } = fauxModels();
    const dir = scratch();
    const home = await testHome(dir, { models });
    const mill = await addHamlet(home, "mill", "miller");
    const scout = await addAgent(home, "home", "scout");
    const chat = await chatWith(home, scout);
    await writeFile(
        `${houseOf(home.layout, scout)}/NOTES.md`,
        "I survived the move",
    );
    const moved = { hamlet: "mill", number: 1 };
    await mkdir(houseOf(home.layout, moved), { recursive: true });
    await writeFile(`${houseOf(home.layout, moved)}/blocked`, "Do not clobber");
    await call(home, faux, MAYOR, "agree_move", {
        agent: "scout",
        to: "mill",
        reason: "Learn",
    });
    expect(
        await call(home, faux, mill, "agree_move", {
            agent: "scout",
            to: "mill",
            reason: "Welcome",
        }),
    ).toContain("move is still pending");
    expect((await home.harness.snapshot(RealmDoc, context))?.pending).toEqual({
        from: scout,
        to: moved,
    });
    await home.close(context);
    await rm(houseOf(home.layout, moved), { recursive: true });
    const again = await testHome(dir, { models });
    expect(
        (await again.harness.snapshot(RealmDoc, context))?.pending,
    ).toBeNull();
    expect(
        await readFile(`${houseOf(again.layout, moved)}/NOTES.md`, "utf8"),
    ).toBe("I survived the move");
    expect((await chatWith(again, moved)).id).toBe(chat.id);
    expect(
        JSON.stringify(await transcript(await chatWith(again, mill))),
    ).toContain("moved from `home` to `mill`");
});
