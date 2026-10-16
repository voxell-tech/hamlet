import { randomUUID } from "node:crypto";
import { fauxAssistantMessage } from "@earendil-works/pi-ai/providers/faux";
import { expect, it } from "vitest";
import { request, served } from "./connection-support.ts";
import { addAgent, addHamlet, context, fauxModels } from "./support.ts";

it("edits and reads another hamlet's limits, character, notes, prompt and history over the protocol", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    await addHamlet(run.home, "mill", "miller");
    const agent = { hamlet: "mill", agent: "mayor" };
    for (const edit of [
        { doc: "character" as const, agent, text: "Be curious." },
        { doc: "notes" as const, agent, text: "# Notes\nKeep flour dry." },
        {
            doc: "limits" as const,
            hamlet: "mill",
            set: { section: "budget", key: "cost", value: 8 },
        },
    ])
        expect(
            await request(run.connection, run.messages, {
                type: "edit",
                id: randomUUID(),
                ...edit,
            }),
        ).toMatchObject({ type: "reply", kind: "notice", text: "Saved." });
    const read = (what: "papers" | "prompt" | "history") =>
        request(run.connection, run.messages, {
            type: "read",
            id: randomUUID(),
            what,
            agent,
        });
    expect(await read("papers")).toMatchObject({
        type: "read",
        papers: {
            character: "Be curious.",
            notes: "# Notes\nKeep flour dry.",
            skills: [],
        },
    });
    expect(await read("prompt")).toMatchObject({
        type: "read",
        prompt: expect.stringContaining("Be curious."),
    });
    faux.setResponses([fauxAssistantMessage("Dry flour.")]);
    await request(run.connection, run.messages, {
        type: "say",
        id: randomUUID(),
        to: agent,
        text: "What matters?",
    });
    expect(await read("history")).toMatchObject({
        type: "read",
        history: [
            { number: 1, current: true, title: "What matters?", messages: 2 },
        ],
    });
    const { LimitsDoc } = await import("../src/limits.ts");
    expect(
        (await run.home.harness.snapshot(LimitsDoc, "mill", context))?.budget
            .cost,
    ).toBe(8);
    expect(
        (await run.home.harness.snapshot(LimitsDoc, "home", context))?.budget
            .cost,
    ).toBe(5);
});

it("refuses papers and edits of removed hamlets and departed agents with their farewell", async () => {
    const run = await served({ models: fauxModels().models });
    await addHamlet(run.home, "mill", "miller");
    const scout = await addAgent(run.home, "mill", "scout");
    const { RecordDoc } = await import("../src/record.ts");
    await run.home.harness.commit(async (tx) => {
        const record = await tx.doc(RecordDoc, "mill", "mill");
        const agent = record.agents.find((a) => a.number === scout.number);
        if (!agent) throw new Error("no scout");
        record.archive.push({
            id: "scout-gone",
            agent,
            how: "removed",
            into: null,
            at: 1,
            by: "mayor",
            reason: "Finished",
            roads: [],
            merge: null,
            back: null,
        });
        record.agents = record.agents.filter((a) => a.number !== scout.number);
    }, context);
    for (const operation of [
        {
            type: "edit" as const,
            doc: "character" as const,
            agent: { hamlet: "mill", agent: "scout" },
            text: "Changed",
        },
        {
            type: "read" as const,
            what: "papers" as const,
            agent: { hamlet: "mill", agent: "scout" },
        },
    ])
        expect(
            await request(run.connection, run.messages, {
                id: randomUUID(),
                ...operation,
            }),
        ).toMatchObject({
            text: expect.stringContaining(
                'was removed from the hamlet by the mayor ("Finished")',
            ),
        });
    await request(run.connection, run.messages, {
        type: "say",
        id: randomUUID(),
        to: { hamlet: "home", agent: "mayor" },
        text: "/remove-hamlet mill Done",
    });
    for (const operation of [
        {
            type: "edit" as const,
            doc: "limits" as const,
            hamlet: "mill",
            set: { section: "budget", key: "cost", value: 8 },
        },
        {
            type: "edit" as const,
            doc: "notes" as const,
            agent: { hamlet: "mill", agent: "mayor" },
            text: "Changed",
        },
        {
            type: "read" as const,
            what: "prompt" as const,
            agent: { hamlet: "mill", agent: "mayor" },
        },
    ])
        expect(
            await request(run.connection, run.messages, {
                id: randomUUID(),
                ...operation,
            }),
        ).toMatchObject({ text: expect.stringContaining("was removed") });
});

it("retains a mill agent's pinned model when a refreshed endpoint no longer lists it, including after restart", async () => {
    const { openaiFake } = await import("./openai-fake.ts");
    const { testHome, scratch, chatWith } = await import("./support.ts");
    const { editLimits } = await import("../src/limits/edit.ts");
    const { refreshModels } = await import("../src/models.ts");
    const { submitLine } = await import("../src/chats.ts");
    const { vi } = await import("vitest");
    const endpoint = await openaiFake();
    endpoint.listing({ data: [{ id: "faux-1" }, { id: "faux-2" }] });
    const dir = scratch();
    const options = {
        env: {
            HAMLET_MODEL: "local::faux-1",
            HAMLET_BASE_URL: endpoint.url,
            HAMLET_REFLECT_EVERY: "0",
        },
    };
    let home = await testHome(dir, options);
    await addHamlet(home, "mill", "miller");
    const scout = await addAgent(home, "mill", "scout");
    await editLimits(
        home,
        "mill",
        { set: { section: "pinned", key: "scout", value: "local::faux-2" } },
        context,
    );
    await chatWith(home, scout);
    endpoint.hold();
    const submitted = await submitLine(
        home,
        { agent: scout, chief: home.chief },
        { id: randomUUID(), text: "Keep working" },
        context,
    );
    await vi.waitFor(() => expect(endpoint.requests).toHaveLength(1), {
        timeout: 10000,
    });
    expect(endpoint.requests[0]?.model).toBe("faux-2");
    endpoint.listing({ data: [{ id: "faux-1" }] });
    await refreshModels(home, context);
    await home.close(context);
    endpoint.release();
    home = await testHome(dir, options);
    expect(home.models.getModel("local", "faux-2")).toBeDefined();
    const resumed = await home.harness.submission(submitted.id, context);
    expect((await resumed?.wait(context))?.status).toBe("done");
    expect(endpoint.requests.at(-1)?.model).toBe("faux-2");
}, 20000);

it("starts the CLI at mayor@mill and switches back with /hamlet home", async () => {
    const { fauxAgents } = await import("./support.ts");
    const { hamlet } = await import("./cli-support.ts");
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    await addHamlet(run.home, "mill", "miller");
    await addAgent(run.home, "mill", "smith");
    await addAgent(run.home, "home", "scout");
    fauxAgents(faux, {
        "home:0": [fauxAssistantMessage("Home answer.")],
        "mill:0": [fauxAssistantMessage("Mill answer.")],
    });
    const cli = hamlet(
        ["chat", "--to", "mayor@mill"],
        run.home.dir,
        "Mill?\n/hamlet home\nHome?\n",
        {
            HAMLET_SERVER: run.server.url,
            HAMLET_TOKEN: run.home.credentials.token,
        },
    );
    expect(await cli.ended).toBe(0);
    expect(cli.out()).toContain("Mill answer.");
    expect(cli.out()).toContain("Home answer.");
    expect(cli.err()).toContain("talking to the mayor of home");
    fauxAgents(faux, { "mill:0": [fauxAssistantMessage("Named answer.")] });
    const env = {
        HAMLET_SERVER: run.server.url,
        HAMLET_TOKEN: run.home.credentials.token,
    };
    const named = hamlet(
        ["chat", "--to", "miller"],
        run.home.dir,
        "Named?\n",
        env,
    );
    expect(await named.ended).toBe(0);
    expect(named.out()).toContain("Named answer.");
    const switched = hamlet(
        ["chat"],
        run.home.dir,
        "/agent smith\n/agent miller\n/hamlet mill\n/agent scout\n/agent missing\n",
        env,
    );
    expect(await switched.ended).toBe(0);
    expect(switched.err()).toContain("talking to the agent smith");
    expect(switched.err()).toContain("talking to the agent miller");
    expect(switched.err()).toContain("talking to the agent scout");
    expect(switched.err()).toContain("error: no agent named `missing`");
    expect(switched.err()).not.toContain("error: no agent named `smith`");
    expect(switched.err()).not.toContain("error: no agent named `miller`");
    expect(switched.err()).not.toContain("error: no agent named `scout`");
    const { join } = await import("node:path");
    const { writeFile } = await import("node:fs/promises");
    const script = join(run.home.dir, "edit-limits.mjs");
    await writeFile(
        script,
        'import {readFileSync,writeFileSync} from "node:fs"; const file=process.argv[2]; const doc=JSON.parse(readFileSync(file,"utf8")); doc.budget.cost=9; writeFileSync(file,JSON.stringify(doc));',
    );
    const edited = hamlet(
        ["edit", "limits", "--hamlet", "mill"],
        run.home.dir,
        "",
        { ...env, EDITOR: `${process.execPath} ${script}` },
    );
    expect(await edited.ended).toBe(0);
    const { LimitsDoc } = await import("../src/limits.ts");
    expect(
        (await run.home.harness.snapshot(LimitsDoc, "mill", context))?.budget
            .cost,
    ).toBe(9);
    expect(
        (await run.home.harness.snapshot(LimitsDoc, "home", context))?.budget
            .cost,
    ).toBe(5);
}, 20000);
