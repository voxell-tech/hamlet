import { access, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fauxAssistantMessage } from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { localDay } from "../src/home/clock.ts";
import { houseOf } from "../src/home/layout.ts";
import { ledgerOf } from "../src/spend.ts";
import {
    agents,
    deed,
    read,
    roadEnds,
    society,
    tool,
    yes,
} from "./acceptance-support.ts";
import { hamlet } from "./cli-support.ts";
import { joined } from "./connection-support.ts";
import { context, fauxAgents, scratch } from "./support.ts";

vi.setConfig({ testTimeout: 30000 });

it("founds a hamlet by vote, pays for asking its mayor, and overturns the founding", async () => {
    const run = await society();
    await agents(run);
    fauxAgents(run.faux, {
        "home:1": [
            tool("call_vote", {
                kind: "found_hamlet",
                hamlet: "mill",
                mayor: "miller",
                purpose: "Grind flour",
                reason: "A new trade",
            }),
            fauxAssistantMessage("A new hamlet"),
        ],
        errands: [fauxAssistantMessage("Let them decide"), yes(), yes(), yes()],
    });
    await deed(run, "Propose a mill", { hamlet: "home", agent: "scout" });
    const founded = await joined(run.server.url, run.home.credentials.token);
    expect(founded.state.hamlets.map((h) => h.id)).toContain("mill");
    expect(
        founded.state.agents.find((a) => a.ref.hamlet === "mill"),
    ).toMatchObject({ name: "miller", mayor: true });
    await access(join(run.home.layout.houses, "mill/0/NOTES.md"));
    fauxAgents(run.faux, {
        "home:0": [
            tool("ask_agent", {
                name: "mayor@mill",
                message: "How much flour?",
            }),
            fauxAssistantMessage("The mill is working"),
        ],
        "mill:0": [fauxAssistantMessage("Three sacks")],
    });
    await deed(run, "Ask the mill");
    const paid = await ledgerOf(
        run.home,
        "home",
        localDay(run.home.now()),
        context,
    );
    expect(paid).toEqual(
        expect.arrayContaining([
            expect.objectContaining({
                what: "model",
                of: "mill",
                agent: 0,
                micros: expect.any(Number),
            }),
        ]),
    );
    expect(
        (
            await ledgerOf(run.home, "mill", localDay(run.home.now()), context)
        ).filter((l) => l.what !== "reset"),
    ).toEqual([]);
    expect(await deed(run, "/overturn 1 Stay together")).toMatchObject({
        kind: "notice",
        text: expect.stringContaining("removed and archived"),
    });
    const closed = await joined(run.server.url, run.home.credentials.token);
    expect(closed.state.closed).toEqual([
        expect.objectContaining({ id: "mill" }),
    ]);
    expect(closed.state.hamlets.map((h) => h.id)).not.toContain("mill");
});

it("agrees a cross-hamlet road and a move, carrying the house and chief conversation", async () => {
    const run = await society();
    await agents(run);
    await deed(run, "/found mill miller Grind flour");
    fauxAgents(run.faux, {
        "mill:0": [
            tool("create_agent", {
                name: "mason",
                purpose: "Build the mill",
                reason: "Stonework",
            }),
            fauxAssistantMessage("Mason ready"),
        ],
    });
    await deed(run, "Make a mason", { hamlet: "mill", agent: "mayor" });
    const scout = { hamlet: "home", agent: "scout" };
    fauxAgents(run.faux, {
        "home:1": [fauxAssistantMessage("My old chief conversation")],
    });
    await deed(run, "Remember this", scout);
    const number = (await read(run, scout)).number;
    await writeFile(
        join(
            houseOf(run.home.layout, { hamlet: "home", number: 1 }),
            "NOTES.md",
        ),
        "# Notes\nIron in the hill",
    );
    fauxAgents(run.faux, {
        "home:0": [
            tool("agree_road", { a: "scout", b: "mason", reason: "Trade" }),
            fauxAssistantMessage("Road proposed"),
            tool("agree_move", {
                agent: "scout",
                to: "mill",
                keep_roads: ["smith"],
                reason: "Learn",
            }),
            fauxAssistantMessage("Move proposed"),
        ],
        "mill:0": [
            fauxAssistantMessage("Considering the road"),
            tool("agree_road", { a: "mason", b: "scout", reason: "Trade" }),
            fauxAssistantMessage("Road agreed"),
            fauxAssistantMessage("Considering the move"),
            tool("agree_move", {
                agent: "scout",
                to: "mill",
                keep_roads: ["smith"],
                reason: "Welcome",
            }),
            fauxAssistantMessage("Move agreed"),
        ],
    });
    await deed(run, "Propose the road");
    await deed(run, "Agree the road", { hamlet: "mill", agent: "mayor" });
    expect(
        roadEnds(
            (await joined(run.server.url, run.home.credentials.token)).state,
        ),
    ).toContain("home#1--mill#1");
    await deed(run, "Propose the move");
    await deed(run, "Agree the move", { hamlet: "mill", agent: "mayor" });
    const moved = { hamlet: "mill", agent: "scout" };
    expect((await read(run, moved)).items.map((i) => i.text)).toContain(
        "My old chief conversation",
    );
    expect((await read(run, moved)).number).toBe(number);
    expect(
        await readFile(join(run.home.layout.houses, "mill/2/NOTES.md"), "utf8"),
    ).toBe("# Notes\nIron in the hill");
    const state = (await joined(run.server.url, run.home.credentials.token))
        .state;
    expect(
        state.agents.find((a) => a.name === "scout" && !a.left)?.ref,
    ).toEqual({ hamlet: "mill", number: 2 });
    expect(roadEnds(state)).toContain("home#2--mill#2");
    expect(roadEnds(state)).not.toContain("home#1--mill#1");
});

it("collects three mayor requests and lets only the chief close, restore and erase the hamlet", async () => {
    const run = await society();
    for (const [id, mayor] of [
        ["mill", "miller"],
        ["dock", "docker"],
        ["farm", "farmer"],
        ["fort", "keeper"],
    ]) {
        expect(await deed(run, `/found ${id} ${mayor} Work`)).toMatchObject({
            kind: "notice",
        });
    }
    for (const place of ["dock", "farm", "fort"]) {
        fauxAgents(run.faux, {
            [`${place}:0`]: [
                tool("ask_removal", { hamlet: "mill", reason: "Idle" }),
                fauxAssistantMessage("Asked"),
            ],
        });
        await deed(run, "Ask for removal", { hamlet: place, agent: "mayor" });
    }
    expect(
        (
            await joined(run.server.url, run.home.credentials.token)
        ).state.hamlets.map((h) => h.id),
    ).toContain("mill");
    expect(
        (await read(run, { hamlet: "mill", agent: "mayor" })).items
            .map((i) => i.text)
            .join("\n"),
    ).toContain("/remove-hamlet mill confirms it");
    expect(await deed(run, "/remove-hamlet mill Finished")).toMatchObject({
        kind: "notice",
    });
    expect(
        (
            await joined(run.server.url, run.home.credentials.token)
        ).state.closed?.map((h) => h.id),
    ).toEqual(["mill"]);
    expect(await deed(run, "/restore-hamlet mill")).toMatchObject({
        kind: "notice",
    });
    expect(
        (await joined(run.server.url, run.home.credentials.token)).state.closed,
    ).toEqual([]);
    await deed(run, "/remove-hamlet mill Finished again");
    expect(await deed(run, "/erase-hamlet mill")).toMatchObject({
        kind: "notice",
    });
    await expect(
        access(join(run.home.layout.houses, "mill")),
    ).rejects.toThrow();
    expect(await deed(run, "/found mill another Work")).toMatchObject({
        kind: "error",
        text: expect.stringContaining("used before"),
    });
});

it("the CLI follows whole realm states while founding, closing and restoring hamlets", async () => {
    const run = await society();
    await deed(run, "/found mill miller Grind flour");
    fauxAgents(run.faux, {
        "mill:0": [
            fauxAssistantMessage("Flour from mill"),
            fauxAssistantMessage("Back in mill"),
        ],
        "home:0": [fauxAssistantMessage("Home again")],
    });
    const cli = hamlet(["chat", "--to", "mayor@mill"], scratch(), undefined, {
        HAMLET_SERVER: run.server.url,
        HAMLET_TOKEN: run.home.credentials.token,
    });
    cli.child.stdin.write("How is the mill?\n");
    await vi.waitFor(() => expect(cli.out()).toContain("Flour from mill"));
    cli.child.stdin.write("/hamlet home\nHello home\n");
    await vi.waitFor(() => expect(cli.out()).toContain("Home again"));
    await deed(run, "/remove-hamlet mill Finished");
    // A request on the CLI connection is the barrier for its received whole state.
    cli.child.stdin.write("/hamlets\n");
    await vi.waitFor(() => expect(cli.err()).toContain("- home (`home`)"));
    cli.child.stdin.write("/hamlet mill\n");
    await vi.waitFor(() =>
        expect(cli.err()).toContain("error: no hamlet named `mill`"),
    );
    await deed(run, "/restore-hamlet mill");
    cli.child.stdin.write("/hamlets\n");
    await vi.waitFor(() => expect(cli.err()).toContain("- mill (`mill`)"));
    cli.child.stdin.write("/hamlet mill\nWelcome back\n");
    await vi.waitFor(() => expect(cli.out()).toContain("Back in mill"));
    cli.child.stdin.end();
    expect(await cli.ended).toBe(0);
});
