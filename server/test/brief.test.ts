import { fauxAssistantMessage } from "@earendil-works/pi-ai/providers/faux";
import { expect, it } from "vitest";
import { submitLine } from "../src/chats.ts";
import { InstructionsDoc } from "../src/instructions.ts";
import { LimitsDoc } from "../src/limits.ts";
import { RecordDoc } from "../src/record.ts";
import { requestReflection } from "../src/reflection.ts";
import {
    addAgent,
    chatWith,
    context,
    fauxModels,
    request,
    say,
    scratch,
    testHome,
} from "./support.ts";

it("tells the mayor its agents and instructions, and an agent its roads and decisions", async () => {
    let now = 30 * 86400000;
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models, now: () => now });
    const scout = await addAgent(home, "home", "scout", "Finds things");
    await addAgent(home, "home", "smith", "Builds things");
    await home.harness.commit(async (tx) => {
        (await tx.doc(InstructionsDoc, "home:1", "home:1")).given.push({
            task: "t",
            chief: "alice",
            text: "Explore carefully",
            at: now,
            done: null,
            hearing: { agent: false, mayor: false, cleared: false },
        });
        const record = await tx.doc(RecordDoc, "home", "home");
        record.decisions.push({
            kind: "created",
            name: "smith",
            purpose: "Builds",
            at: now,
            agent: 2,
            by: "mayor",
            reason: "Needed",
        });
        record.decisions.push({
            kind: "road_refused",
            from: 1,
            to: "smith",
            at: now,
            by: "mayor",
            reason: "Not yet",
        });
        record.decisions.push({
            kind: "road_refused",
            from: 2,
            to: "scout",
            at: now,
            by: "mayor",
            reason: "Other refusal",
        });
    }, context);
    let section = "";
    const inspect = () => {
        faux.setResponses([
            (prompt, _options, _state, model) => {
                section = request(prompt.messages, model).sections.hamlet ?? "";
                return fauxAssistantMessage("ok");
            },
        ]);
    };
    inspect();
    await (await say(home, "Hello")).wait(context);
    expect(section).toContain("scout: Finds things");
    expect(section).toContain("alice told scout: Explore carefully");
    expect(section).toContain("call_vote");
    expect(section).not.toContain("commands can read each agent");
    inspect();
    await (
        await submitLine(
            home,
            { agent: scout, chief: home.chief },
            { id: "first", text: "Hello" },
            context,
        )
    ).wait(context);
    expect(section).toContain("Your roads lead to: mayor");
    expect(section).toContain("The mayor created `smith`: Needed");
    expect(section).toContain("you asked for to `smith`: Not yet");
    expect(section).not.toContain("Other refusal");
    now += 8 * 86400000;
    inspect();
    await (
        await submitLine(
            home,
            { agent: scout, chief: home.chief },
            { id: "quiet", text: "Hello" },
            context,
        )
    ).wait(context);
    expect(section).not.toContain("# How your hamlet was run lately");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).mayor.toldDays = 60;
    }, context);
    inspect();
    await (
        await submitLine(
            home,
            { agent: scout, chief: home.chief },
            { id: "longer", text: "Hello" },
            context,
        )
    ).wait(context);
    expect(section).toContain("The mayor created `smith`: Needed");
});

it("tells a kept agent 'merged into you' and others who to talk to", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const smith = await addAgent(home, "home", "smith");
    await home.harness.commit(async (tx) => {
        const record = await tx.doc(RecordDoc, "home", "home");
        record.archive.push({
            id: "cook",
            agent: {
                number: 2,
                name: "cook",
                purpose: "Cooked",
                since: 0,
                model: null,
            },
            how: "merged",
            into: smith.number,
            at: home.now(),
            by: "mayor",
            reason: "Same work",
            roads: [],
            merge: null,
            back: null,
        });
    }, context);
    let section = "";
    const inspect = () => {
        faux.setResponses([
            (prompt, _options, _state, model) => {
                section = request(prompt.messages, model).sections.hamlet ?? "";
                return fauxAssistantMessage("ok");
            },
        ]);
    };
    inspect();
    await (
        await submitLine(
            home,
            { agent: smith, chief: home.chief },
            { id: "read", text: "Hello" },
            context,
        )
    ).wait(context);
    expect(section).toContain("cook: merged into you");
    inspect();
    await (await say(home, "Hello")).wait(context);
    expect(section).toContain("cook: merged into smith; talk to smith");
});

it("adds the civic reflection question for an agent, but not for the mayor", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, "home", "scout");
    for (const [ref, resident] of [
        [{ hamlet: "home", number: 0 }, false],
        [scout, true],
    ] as const) {
        const conversation = await chatWith(home, ref);
        faux.setResponses([
            (prompt) => {
                expect(
                    JSON.stringify(prompt).includes(
                        "how the hamlet is run get in the way",
                    ),
                ).toBe(resident);
                return fauxAssistantMessage("None");
            },
        ]);
        const sub = await requestReflection(
            home,
            conversation.id,
            context,
            "explicit",
        );
        expect((await sub?.wait(context))?.status).toBe("done");
    }
});
