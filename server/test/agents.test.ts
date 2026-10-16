import { readFile } from "node:fs/promises";
import { join } from "node:path";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it } from "vitest";
import { CharacterDoc } from "../src/character.ts";
import { houseOf } from "../src/home.ts";
import { LimitsDoc } from "../src/limits.ts";
import { agentKey, RecordDoc } from "../src/record.ts";
import {
    context,
    fauxModels,
    mayorChat,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

it("creates a normalized agent with character, allowed model and charged road", async () => {
    const { models, faux } = fauxModels(["faux-1", "faux-2"]);
    const home = await testHome(scratch(), { models });
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).models = ["faux::faux-2"];
    }, context);
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("create_agent", {
                name: "smith",
                purpose: "Builds",
                reason: "Needed",
            }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage(
            fauxToolCall("create_agent", {
                name: "FelineBiologist",
                purpose: "Looks",
                reason: "Needed",
                roads: ["smith"],
                model: "faux::faux-2",
                character: "Careful",
            }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("Done"),
    ]);
    expect((await (await say(home, "Create them")).wait(context)).status).toBe(
        "done",
    );
    const record = await home.harness.snapshot(RecordDoc, "home", context);
    expect(record?.agents[2]).toMatchObject({
        name: "feline-biologist",
        model: "faux::faux-2",
    });
    expect(record?.roads).toHaveLength(1);
    const ref = { hamlet: "home", number: 2 };
    expect(
        (await home.harness.snapshot(CharacterDoc, agentKey(ref), context))
            ?.text,
    ).toBe("Careful");
    expect(
        await readFile(join(houseOf(home.layout, ref), "NOTES.md"), "utf8"),
    ).toBe("# Notes\n");
});

it("refuses reserved names, blank purposes, duplicate names and disallowed models in words", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const calls = [
        { name: "mayor", purpose: "x" },
        { name: "founder", purpose: "x" },
        { name: "all", purpose: "x" },
        { name: "bad1", purpose: "x" },
        { name: "scout", purpose: " " },
        { name: "scout", purpose: "Looks" },
        { name: "scout", purpose: "Again" },
    ];
    faux.setResponses([
        ...calls.map((args) =>
            fauxAssistantMessage(
                fauxToolCall("create_agent", { ...args, reason: "Needed" }),
                { stopReason: "toolUse" },
            ),
        ),
        fauxAssistantMessage(
            fauxToolCall("set_model", {
                name: "scout",
                model: "no::such",
                reason: "Try",
            }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("Done"),
    ]);
    await (await say(home, "Try")).wait(context);
    expect(
        (await home.harness.snapshot(RecordDoc, "home", context))?.agents.map(
            (a) => a.name,
        ),
    ).toEqual(["founder", "scout"]);
    const text = JSON.stringify(await transcript(await mayorChat(home)));
    expect(text).toContain("already exists");
    expect(text).toContain("may not run");
    expect(text).toContain("say what the agent is for");
});

it("holds reserved creation for allow and refuses it on deny", async () => {
    const { answer } = await import("../src/gate.ts");
    const { openWaits } = await import("./support.ts");
    const { vi } = await import("vitest");
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).mayor.createAgents = false;
    }, context);
    for (const choice of ["allow", "deny"] as const) {
        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("create_agent", {
                    name: choice === "allow" ? "scout" : "smith",
                    purpose: "Looks",
                    reason: "Needed",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("Done"),
        ]);
        const sub = await say(home, "Create");
        await vi.waitFor(
            async () => expect(await openWaits(home)).toHaveLength(1),
            { timeout: 10000 },
        );
        const wait = (await openWaits(home))[0];
        if (!wait) throw new Error("No wait");
        expect(wait.danger).toBe("reserved");
        await answer(
            home,
            { wait: wait.id, choice, chief: home.chief },
            context,
        );
        await sub.wait(context);
    }
    expect(
        (await home.harness.snapshot(RecordDoc, "home", context))?.agents.map(
            (a) => a.name,
        ),
    ).toEqual(["founder", "scout"]);
});

it("keeps a mayor's model choice by number and uses it on the agent's next request", async () => {
    const { submitLine } = await import("../src/chats.ts");
    const { addAgent, chatWith } = await import("./support.ts");
    const { models, faux } = fauxModels(["faux-1", "faux-2"]);
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, "home", "scout");
    await chatWith(home, scout);
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).models = ["faux::faux-2"];
    }, context);
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("set_model", {
                name: "scout",
                model: "faux::faux-2",
                reason: "Needs care",
            }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("Done"),
    ]);
    await (await say(home, "Choose")).wait(context);
    faux.setResponses([
        (_prompt, _options, _state, model) => {
            expect(model.id).toBe("faux-2");
            return fauxAssistantMessage("Ready");
        },
    ]);
    expect(
        (
            await (
                await submitLine(
                    home,
                    { agent: scout, chief: home.chief },
                    { id: "model", text: "Work" },
                    context,
                )
            ).wait(context)
        ).status,
    ).toBe("done");
    expect(
        (await home.harness.snapshot(RecordDoc, "home", context))?.decisions,
    ).toContainEqual(
        expect.objectContaining({
            kind: "model",
            agent: scout.number,
            reason: "Needs care",
        }),
    );
});
