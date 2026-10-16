import { randomUUID } from "node:crypto";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it } from "vitest";
import { submitLine } from "../src/chats.ts";
import type { Home } from "../src/home.ts";
import { LimitsDoc } from "../src/limits.ts";
import { RealmDoc, realmRoads } from "../src/realm.ts";
import type { AgentRef } from "../src/record.ts";
import { RecordDoc } from "../src/record.ts";
import { VotesDoc } from "../src/votes.ts";
import {
    addAgent,
    addHamlet,
    chatWith,
    context,
    deedWords,
    type Faux,
    fauxAgents,
    fauxModels,
    livingAgents,
    MAYOR,
    request,
    scratch,
    testHome,
    transcript,
    usedRoad,
} from "./support.ts";

async function call(
    home: Home,
    faux: Faux,
    mayor: AgentRef,
    tool: string,
    args: Parameters<typeof fauxToolCall>[1],
) {
    const realm = await home.harness.snapshot(RealmDoc, context);
    const scripts: Parameters<typeof fauxAgents>[1] = {};
    for (const hamlet of realm?.hamlets ?? []) {
        const record = await home.harness.snapshot(
            RecordDoc,
            hamlet.id,
            context,
        );
        if (record)
            scripts[`${hamlet.id}:${record.mayor}`] = [
                fauxAssistantMessage("Agreed to consider it"),
            ];
    }
    scripts[`${mayor.hamlet}:${mayor.number}`] = [
        fauxAssistantMessage(fauxToolCall(tool, args), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("done"),
    ];
    fauxAgents(faux, scripts);
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
        JSON.stringify(await transcript(await chatWith(home, MAYOR))),
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
    const { houseOf } = await import("../src/home/layout.ts");
    const { CharacterDoc } = await import("../src/prompt.ts");
    const { agentKey } = await import("../src/record.ts");
    const { give, GrantsDoc, grantsOf } = await import("../src/gate.ts");
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
            await usedRoad(tx, "home", scout.number, b, {
                messages: 4,
                at: home.now(),
                since: 1,
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
    const notices = (await transcript(await chatWith(home, MAYOR))).filter(
        (entry) => entry.kind === "hamlet.notice",
    );
    expect(JSON.stringify(notices)).toContain("You agreed that `scout` moves");
    expect(JSON.stringify(notices)).toContain(
        "`scout` moved from `home` to `mill` with its house.",
    );
    const moved = { hamlet: "mill", number: 2 };
    expect(
        (await livingAgents(home, "home")).some((a) => a.name === "scout"),
    ).toBe(false);
    expect(
        (await livingAgents(home, "mill")).find((a) => a.name === "scout"),
    ).toMatchObject({ ref: { hamlet: "mill", number: 2 }, purpose: "" });
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
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).hamlets.roadsBetween = false;
    }, context);
    expect(await road(MAYOR, "baker", "smith")).toContain("allow no roads");
    expect(await move(MAYOR, "founder")).toContain("mayor never leaves");
    expect(await move(dock, "scout")).toContain("only the mayor");
    expect(await move(mill, "smith")).toContain("already lives");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "mill", "mill")).mostAgents = 1;
    }, context);
    expect(await move(MAYOR, "scout")).toContain("is full");
}, 30000);

it("finishes a committed house move on reopening without losing the chief's chat", async () => {
    const { mkdir, writeFile, rm, readFile } = await import("node:fs/promises");
    const { houseOf } = await import("../src/home/layout.ts");
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

async function openVote(home: Home, hamlet: string) {
    await home.harness.commit(async (tx) => {
        (await tx.doc(VotesDoc, hamlet, hamlet)).votes.push({
            id: 1,
            motion: { kind: "replace_mayor", candidate: 1 },
            by: { chief: home.chief },
            reason: "Change",
            mayor: 0,
            voters: [1],
            answer: null,
            ballots: [],
            standing: "open",
            called: home.now(),
            task: "vote",
        });
    }, context);
}

it.each(["agree_road", "agree_move"])(
    "answers %s while a stored vote is open without blocking later deeds",
    async (tool) => {
        const { models, faux } = fauxModels();
        const dir = scratch();
        let home = await testHome(dir, { models });
        await addHamlet(home, "mill", "miller");
        await addAgent(home, "home", "scout");
        await addAgent(home, "mill", "smith");
        await openVote(home, "home");
        await home.close(context);
        home = await testHome(dir, { models });
        const result = await call(
            home,
            faux,
            MAYOR,
            tool,
            tool === "agree_road"
                ? { a: "scout", b: "smith", reason: "Trade" }
                : { agent: "scout", to: "mill", reason: "Learn" },
        );
        expect(result).toContain("vote 1 is open");
        await home.harness.commit(async (tx) => {
            const place = (await tx.doc(RealmDoc)).hamlets.find(
                (h) => h.id === "mill",
            );
            if (!place) throw new Error("no mill");
            place.title = "Still answering";
        }, context);
        expect(
            (await livingAgents(home, "home")).some((a) => a.name === "scout"),
        ).toBe(true);
        expect((await home.harness.snapshot(RealmDoc, context))?.roads).toEqual(
            [],
        );
    },
);

it.each(["home", "mill"])(
    "holds a move when the other mayor's hamlet %s opens a vote",
    async (hamlet) => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), { models });
        const mill = await addHamlet(home, "mill", "miller");
        const scout = await addAgent(home, "home", "scout");
        const args = { agent: "scout", to: "mill", reason: "Learn" };
        const first = hamlet === "home" ? MAYOR : mill;
        const second = hamlet === "home" ? mill : MAYOR;
        expect(await call(home, faux, first, "agree_move", args)).toContain(
            "other mayor has not yet",
        );
        await openVote(home, hamlet);
        expect(await call(home, faux, second, "agree_move", args)).toContain(
            "vote 1 is open",
        );
        expect(await livingAgents(home, "home")).toContainEqual(
            expect.objectContaining({
                ref: { hamlet: "home", number: scout.number },
                name: "scout",
            }),
        );
        expect(
            (
                await home.harness.snapshot(RecordDoc, "mill", context)
            )?.decisions.some((d) => d.kind === "moved_in"),
        ).toBe(false);
        expect(
            (await home.harness.snapshot(RealmDoc, context))?.pending,
        ).toBeNull();
    },
);

it("charges a moved sender's next question to its new hamlet without replacing the peer conversation", async () => {
    const { models, faux } = fauxModels(["faux-1"], 1);
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    const mill = await addHamlet(home, "mill", "miller");
    const scout = await addAgent(home, "home", "scout");
    const baker = await addAgent(home, "home", "baker");
    await home.harness.commit(async (tx) => {
        await usedRoad(tx, "home", scout.number, baker.number, {
            messages: 0,
            at: 0,
            since: 1,
            by: "chief",
        });
    }, context);
    const ask = async (from: AgentRef) => {
        const steps = [
            fauxAssistantMessage(
                fauxToolCall("ask_agent", {
                    name: "baker",
                    message: "How much flour?",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("Thanks"),
        ];
        fauxAgents(faux, {
            [`${scout.hamlet}:${scout.number}`]: steps,
            [`${from.hamlet}:${from.number}`]: steps,
            [`home:${baker.number}`]: [fauxAssistantMessage("Three sacks")],
        });
        const result = await (
            await submitLine(
                home,
                { agent: from, chief: home.chief },
                { id: randomUUID(), text: "Ask the baker" },
                context,
            )
        ).wait(context);
        expect(result.status).toBe("done");
    };
    await ask(scout);
    const { PeersDoc } = await import("../src/letters.ts");
    const { PayerDoc } = await import("../src/chats.ts");
    const peer = (
        await home.harness.snapshot(PeersDoc, `home:${baker.number}`, context)
    )?.questions[`home:${scout.number}`];
    if (peer === undefined) throw new Error("No question conversation");
    await call(home, faux, MAYOR, "agree_move", {
        agent: "scout",
        to: "mill",
        keep_roads: ["baker"],
        reason: "Learn",
    });
    await call(home, faux, mill, "agree_move", {
        agent: "scout",
        to: "mill",
        keep_roads: ["baker"],
        reason: "Welcome",
    });
    const moved = { hamlet: "mill", number: 1 };
    await ask(moved);
    expect(
        (await home.harness.snapshot(PeersDoc, `home:${baker.number}`, context))
            ?.questions["mill:1"],
    ).toBe(peer);
    expect(await home.harness.snapshot(PayerDoc, peer, context)).toMatchObject({
        hamlet: "mill",
    });
    const { ledgerOf } = await import("./support.ts");
    const { localDay } = await import("../src/home/clock.ts");
    expect(
        await ledgerOf(home, "mill", localDay(home.now()), context),
    ).toContainEqual(
        expect.objectContaining({
            what: "model",
            of: "home",
            agent: baker.number,
            micros: expect.any(Number),
        }),
    );
});

it("keeps an unsplit merge in its hamlet until the chief splits or erases it", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    const mill = await addHamlet(home, "mill", "miller");
    await addAgent(home, "home", "librarian");
    await addAgent(home, "home", "scout");
    const { merge } = await import("../src/agents.ts");
    await merge(
        home,
        {
            hamlet: "home",
            chief: home.chief,
            rest: "scout into librarian Overlap",
        },
        context,
    );
    const args = { agent: "librarian", to: "mill", reason: "Learn" };
    for (const mayor of [MAYOR, mill])
        expect(await call(home, faux, mayor, "agree_move", args)).toContain(
            "split or erase",
        );
    expect((await home.harness.snapshot(RealmDoc, context))?.moves).toEqual([]);
    expect((await livingAgents(home, "home")).map((a) => a.name)).toContain(
        "librarian",
    );
    const { restore: restoreDeed } = await import("../src/agents.ts");
    const restore = deedWords(restoreDeed);
    expect(
        await restore(
            home,
            { hamlet: "home", chief: home.chief, rest: "scout" },
            context,
        ),
    ).toContain("Split");
    await call(home, faux, MAYOR, "agree_move", args);
    expect(await call(home, faux, mill, "agree_move", args)).toContain(
        "moved from",
    );
});

it("agree_move fits two agents beside the receiving mayor and refuses a third with its existing text", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const mill = await addHamlet(home, "mill", "miller");
    await addAgent(home, "mill", "baker", "Bakes");
    await addAgent(home, "home", "scout", "Maps");
    await addAgent(home, "home", "smith", "Builds");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "mill", "mill")).mostAgents = 2;
    }, context);
    const args = { agent: "scout", to: "mill", reason: "Learn" };
    expect(await call(home, faux, MAYOR, "agree_move", args)).toContain(
        "other mayor has not yet",
    );
    expect(await call(home, faux, mill, "agree_move", args)).toContain(
        "moved from",
    );
    expect((await livingAgents(home, "mill")).map((a) => a.name)).toEqual([
        "miller",
        "baker",
        "scout",
    ]);
    for (const mayor of [MAYOR, mill])
        expect(
            await call(home, faux, mayor, "agree_move", {
                ...args,
                agent: "smith",
            }),
        ).toContain("the hamlet `mill` is full");
    expect((await livingAgents(home, "mill")).map((a) => a.name)).toEqual([
        "miller",
        "baker",
        "scout",
    ]);
});

it.each(["agree_road", "agree_move"] as const)(
    "%s refuses a departed name with its farewell",
    async (tool) => {
        const { remove } = await import("../src/agents.ts");
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        await addHamlet(home, "mill", "miller");
        await addAgent(home, "home", "scout");
        await addAgent(home, "mill", "smith");
        await remove(
            home,
            { hamlet: "home", chief: home.chief, rest: "scout Work ended" },
            context,
        );
        const result = await call(
            home,
            faux,
            MAYOR,
            tool,
            tool === "agree_road"
                ? { a: "scout", b: "smith", reason: "Trade" }
                : { agent: "scout", to: "mill", reason: "Learn" },
        );
        const farewell =
            '`scout` was removed from the hamlet by the chief chief ("Work ended"). Its house is archived and its roads went with it; only a chief can restore it.';
        expect(result).toContain(JSON.stringify(farewell).slice(1, -1));
    },
);

it.each([
    { to: "mill", text: "`mayor` already lives in `mill`" },
    { to: "home", text: "a mayor never leaves its hamlet" },
])(
    "agree_move resolves the caller's unqualified office when joining $to",
    async ({ to, text }) => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        const mill = await addHamlet(home, "mill", "miller");
        expect(
            await call(home, faux, mill, "agree_move", {
                agent: "mayor",
                to,
                reason: "Learn",
            }),
        ).toContain(text);
    },
);

it("agree_road refuses an unqualified office as an agent road end", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    const mill = await addHamlet(home, "mill", "miller");
    await addAgent(home, "home", "scout");
    expect(
        await call(home, faux, mill, "agree_road", {
            a: "mayor",
            b: "scout",
            reason: "Trade",
        }),
    ).toContain("name two agents; mayors already have roads to each other");
});

it("returns a blocked pending house move as a classified refusal", async () => {
    const { mkdir, writeFile } = await import("node:fs/promises");
    const { houseOf } = await import("../src/home/layout.ts");
    const { finishMove } = await import("../src/realm/dealings.ts");
    const home = await testHome(scratch(), { models: fauxModels().models });
    await addHamlet(home, "mill", "miller");
    const from = await addAgent(home, "home", "scout");
    const to = { hamlet: "mill", number: 1 };
    await mkdir(houseOf(home.layout, to), { recursive: true });
    await writeFile(`${houseOf(home.layout, to)}/blocked`, "Keep me");
    await home.harness.commit(async (tx) => {
        (await tx.doc(RealmDoc)).pending = { from, to };
    }, context);
    expect(await finishMove(home, context)).toMatchObject({
        ok: false,
        error: {
            kind: "move-house-occupied",
            operation: "finish-move",
            from,
            to,
            retrySafe: false,
        },
    });
});
