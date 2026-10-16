import { randomUUID } from "node:crypto";
import { access, mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fauxAssistantMessage } from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { houseOf } from "../src/home/layout.ts";
import { LimitsDoc } from "../src/limits.ts";
import {
    agents,
    deed,
    read,
    roadEnds,
    society,
    tool,
    yes,
} from "./acceptance-support.ts";
import { joined, request } from "./connection-support.ts";
import {
    chatWith,
    context,
    fauxAgents,
    openWaits,
    transcript,
} from "./support.ts";

vi.setConfig({ testTimeout: 30000 });

it("creates a society with a road, carries a letter and its reply, then asks a question", async () => {
    const run = await society();
    await agents(run);
    fauxAgents(run.faux, {
        "home:0": [
            tool("tell_agent", { name: "smith", message: "Make a hinge" }),
            fauxAssistantMessage("Sent"),
            fauxAssistantMessage("The letter returned"),
        ],
        "home:2": [fauxAssistantMessage("Hinge ready")],
    });
    await deed(run, "Tell smith");
    await vi.waitFor(
        async () =>
            expect((await read(run)).items.map((i) => i.text)).toContain(
                "The letter returned",
            ),
        { timeout: 10000 },
    );
    expect(
        (await read(run)).items.find((i) => i.text.includes("Reply from")),
    ).toMatchObject({
        author: "smith@home",
        text: expect.stringContaining("Hinge ready"),
    });
    fauxAgents(run.faux, {
        "home:0": [
            tool("ask_agent", { name: "scout", message: "Where is the iron?" }),
            fauxAssistantMessage("Iron found"),
        ],
        "home:1": [fauxAssistantMessage("In the northern hill")],
    });
    await deed(run, "Ask scout");
    const late = await joined(run.server.url, run.home.credentials.token);
    const scout = late.state.agents.find((a) => a.name === "scout");
    expect(scout?.conversations?.map((c) => c.kind)).toContain("questions");
    expect(roadEnds(late.state)).toContain("home#1--home#2");
    const peer = await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat: {
            with: { hamlet: "home", agent: "scout" },
            from: { hamlet: "home", number: 0 },
            kind: "questions",
        },
    });
    expect(peer).toMatchObject({
        type: "transcript",
        items: [
            expect.objectContaining({
                author: "founder@home",
                text: "Question from `mayor` (waiting for your answer):\nWhere is the iron?",
            }),
            expect.objectContaining({ text: "In the northern hill" }),
        ],
    });
});

it("asks the mayor for a road and has the mayor build it", async () => {
    const run = await society();
    await agents(run);
    fauxAgents(run.faux, {
        "home:1": [
            tool("request_road", { to: "poet", reason: "Map legends" }),
            fauxAssistantMessage("Road requested"),
        ],
        "home:0": [
            tool("build_road", {
                a: "scout",
                b: "poet",
                reason: "Map legends",
            }),
            fauxAssistantMessage("Road laid"),
        ],
    });
    await deed(run, "Request a road", { hamlet: "home", agent: "scout" });
    await vi.waitFor(
        async () => {
            const late = await joined(
                run.server.url,
                run.home.credentials.token,
            );
            expect(roadEnds(late.state)).toContain("home#1--home#3");
            await late.connection.close();
        },
        { timeout: 10000 },
    );
});

it("protects an instructed agent from the mayor, then removes, refuses letters and undoes with its roads", async () => {
    const run = await society();
    await agents(run);
    let release: (() => void) | undefined;
    const held = new Promise<void>((resolve) => {
        release = resolve;
    });
    fauxAgents(run.faux, {
        "home:1": [
            async () => {
                await held;
                return fauxAssistantMessage("Chief task answered");
            },
        ],
        "home:0": [
            tool("remove_agent", { name: "scout", reason: "Finished" }),
            fauxAssistantMessage("Removal checked"),
        ],
    });
    const working = deed(run, "Keep working", {
        hamlet: "home",
        agent: "scout",
    });
    await vi.waitFor(async () =>
        expect(
            (await read(run, { hamlet: "home", agent: "scout" })).working,
        ).toBe(true),
    );
    await deed(run, "Remove scout");
    expect(
        (await read(run)).items
            .map((i) => i.tool?.failure ?? i.text)
            .join("\n"),
    ).toContain("not finished");
    release?.();
    await working;
    fauxAgents(run.faux, {
        "home:0": [
            tool("remove_agent", { name: "scout", reason: "Finished" }),
            tool("tell_agent", { name: "scout", message: "Still there?" }),
            fauxAssistantMessage("Archived"),
        ],
    });
    await deed(run, "Remove now");
    expect(
        (await read(run)).items
            .map((i) => i.tool?.failure ?? i.text)
            .join("\n"),
    ).toContain("removed");
    expect(await deed(run, "/undo")).toMatchObject({
        kind: "notice",
        text: expect.stringContaining("scout"),
    });
    const late = await joined(run.server.url, run.home.credentials.token);
    expect(
        late.state.agents.find((a) => a.name === "scout")?.left,
    ).toBeUndefined();
    expect(roadEnds(late.state)).toContain("home#1--home#2");
});

it("merges an agent, redirects its letters and splits it back out", async () => {
    const run = await society();
    await agents(run);
    fauxAgents(run.faux, {
        "home:0": [
            tool("merge_agents", {
                keep: "smith",
                absorb: "scout",
                purpose: "Find and forge",
                reason: "Together",
            }),
            tool("tell_agent", { name: "scout", message: "Hello" }),
            fauxAssistantMessage("Merged"),
        ],
        errands: [fauxAssistantMessage("# Notes\nFind and forge.")],
    });
    await deed(run, "Merge the work");
    const late = await joined(run.server.url, run.home.credentials.token);
    expect(
        late.state.agents.find((a) => a.name === "scout")?.left,
    ).toMatchObject({ how: "merged", into: { hamlet: "home", number: 2 } });
    expect(
        JSON.stringify(
            await transcript(
                await chatWith(run.home, { hamlet: "home", number: 0 }),
            ),
        ),
    ).toContain("talk to");
    expect(await deed(run, "/restore scout")).toMatchObject({
        kind: "notice",
        text: expect.stringContaining("Split"),
    });
});

it("holds mayor decisions during a vote, follows its new office and overturns it", async () => {
    const run = await society();
    await agents(run);
    await read(run);
    let release: (() => void) | undefined;
    const held = new Promise<void>((resolve) => {
        release = resolve;
    });
    fauxAgents(run.faux, {
        errands: [
            async () => {
                await held;
                return fauxAssistantMessage("Let the agents decide");
            },
            yes(),
            yes(),
            yes(),
        ],
        "home:0": [
            tool("remove_agent", { name: "scout", reason: "Too soon" }),
            fauxAssistantMessage("Checked"),
        ],
    });
    const voter = await joined(run.server.url, run.home.credentials.token);
    const vote = request(voter.connection, voter.messages, {
        type: "say",
        id: randomUUID(),
        to: { hamlet: "home", agent: "mayor" },
        text: "/vote smith A new direction",
    });
    await vi.waitFor(async () =>
        expect(
            (
                await joined(run.server.url, run.home.credentials.token)
            ).state.hamlets[0]?.votes?.some((v) => v.open),
        ).toBe(true),
    );
    await deed(run, "Remove scout");
    expect(
        JSON.stringify(
            await transcript(
                await chatWith(run.home, { hamlet: "home", number: 0 }),
            ),
        ),
    ).toContain("nothing else is decided");
    release?.();
    expect(await vote).toMatchObject({ kind: "notice" });
    await vi.waitFor(async () =>
        expect(
            (
                await joined(run.server.url, run.home.credentials.token)
            ).state.agents.find((a) => a.name === "smith")?.mayor,
        ).toBe(true),
    );
    fauxAgents(run.faux, {
        "home:2": [fauxAssistantMessage("Smith holds the office")],
    });
    await deed(run, "Who is mayor?");
    expect((await read(run)).items.map((i) => i.text)).toContain(
        "Smith holds the office",
    );
    expect(await deed(run, "/overturn 1 Back")).toMatchObject({
        text: expect.stringContaining("founder"),
    });
});

it("shelves an offered skill in a whole state and includes it in another agent's prompt", async () => {
    const run = await society();
    await agents(run);
    const skill = join(
        houseOf(run.home.layout, { hamlet: "home", number: 1 }),
        "skills",
        "map-iron",
    );
    await mkdir(skill, { recursive: true });
    await writeFile(
        join(skill, "SKILL.md"),
        "---\nname: map-iron\ndescription: Maps iron deposits.\n---\nRead the map.\n",
    );
    fauxAgents(run.faux, {
        "home:1": [
            tool("submit_skill", { skill: "map-iron", reason: "Useful maps" }),
            fauxAssistantMessage("Shared"),
        ],
        errands: [fauxAssistantMessage("ACCEPT Useful maps")],
    });
    await deed(run, "Share the map", { hamlet: "home", agent: "scout" });
    await vi.waitFor(() =>
        expect(
            run.messages.some(
                (m) =>
                    m.type === "state" &&
                    m.library?.some((s) => s.name === "map-iron"),
            ),
        ).toBe(true),
    );
    const prompt = await request(run.connection, run.messages, {
        type: "read",
        id: randomUUID(),
        what: "prompt",
        agent: { hamlet: "home", agent: "smith" },
    });
    expect(prompt).toMatchObject({
        prompt: expect.stringContaining("map-iron: Maps iron deposits."),
    });
    await expect(access(skill)).rejects.toThrow();
});

it("holds a chief instruction for an objection, then goes or withdraws without another tool", async () => {
    const run = await society();
    await agents(run);
    await run.home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).asking.mayorReview = "never";
    }, context);
    for (const choice of ["go", "withdraw"] as const) {
        fauxAgents(run.faux, {
            "home:1": [
                tool("object", { reason: "This is the only copy" }),
                tool("write", { path: `${choice}.txt`, content: "done" }),
                fauxAssistantMessage("Done"),
            ],
        });
        const pending = deed(run, `Delete the ${choice} copy`, {
            hamlet: "home",
            agent: "scout",
        });
        await vi.waitFor(async () =>
            expect(await openWaits(run.home)).toHaveLength(1),
        );
        const wait = (await openWaits(run.home))[0];
        if (!wait) throw new Error("No objection");
        expect(
            await request(run.connection, run.messages, {
                type: "answer",
                id: randomUUID(),
                wait: wait.id,
                choice,
                text: "I checked",
            }),
        ).toMatchObject({ kind: "notice" });
        await pending;
        const file = join(
            houseOf(run.home.layout, { hamlet: "home", number: 1 }),
            `${choice}.txt`,
        );
        if (choice === "go") await access(file);
        else await expect(access(file)).rejects.toThrow();
    }
});
