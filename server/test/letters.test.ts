import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { submitLine } from "../src/chats.ts";
import { LimitsDoc } from "../src/limits.ts";
import { RecordDoc } from "../src/record.ts";
import { PeersDoc } from "../src/society/docs.ts";
import { TaskDoc } from "../src/tasks.ts";
import {
    addAgent,
    addHamlet,
    context,
    fauxAgents,
    fauxModels,
    mayorChat,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

const tool = (name: string, args: Record<string, string>) =>
    fauxAssistantMessage(fauxToolCall(name, args), { stopReason: "toolUse" });

it("delivers slash words as a letter, then a reply under the same capped task", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    const librarian = await addAgent(home, "home", "librarian");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).task.messages = 2;
    }, context);
    fauxAgents(faux, {
        "home:0": [
            tool("tell_agent", { name: "librarian", message: "/new" }),
            fauxAssistantMessage("Sent"),
            (prompt) => {
                expect(JSON.stringify(prompt)).toContain(
                    "Reply from `librarian`: ok",
                );
                return tool("tell_agent", {
                    name: "librarian",
                    message: "again",
                });
            },
            fauxAssistantMessage("Done"),
        ],
        "home:1": [
            (prompt) => {
                expect(JSON.stringify(prompt)).toContain("/new");
                return fauxAssistantMessage("ok");
            },
        ],
    });
    const sub = await say(home, "Tell");
    await sub.wait(context);
    await vi.waitFor(async () =>
        expect(
            JSON.stringify(await transcript(await mayorChat(home))),
        ).toContain("used all 2 of its messages"),
    );
    const peer = (await home.harness.snapshot(PeersDoc, "home:1", context))
        ?.letters["home:0"];
    expect(peer).toBeDefined();
    if (peer === undefined) throw new Error("No peer conversation");
    const entries = await transcript(peer, home.harness);
    expect(entries.filter((e) => e.kind === "hamlet.letter")).toHaveLength(1);
    expect(
        (await home.harness.snapshot(RecordDoc, "home", context))?.roads,
    ).toMatchObject([{ messages: 2, lastFrom: librarian.number }]);
});

it("reuses question conversations, counts answers and refuses tell while awaited", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    await addAgent(home, "home", "scout");
    fauxAgents(faux, {
        "home:0": [
            tool("ask_agent", { name: "scout", message: "one" }),
            tool("ask_agent", { name: "scout", message: "two" }),
            fauxAssistantMessage("Done"),
        ],
        "home:1": [
            tool("tell_agent", { name: "mayor", message: "later" }),
            fauxAssistantMessage("one answer"),
            fauxAssistantMessage("two answer"),
        ],
    });
    const sub = await say(home, "Ask twice");
    expect((await sub.wait(context)).status).toBe("done");
    const peer = (await home.harness.snapshot(PeersDoc, "home:1", context))
        ?.questions["home:0"];
    if (peer === undefined) throw new Error("No peer conversation");
    const entries = await transcript(peer, home.harness);
    expect(entries.filter((e) => e.kind === "hamlet.question")).toHaveLength(2);
    expect(JSON.stringify(entries)).toContain(
        "another agent is waiting on your reply",
    );
    const chat = await mayorChat(home);
    const task = `chief:${chat.id}:`;
    const chief = (await transcript(chat)).find(
        (e) => e.kind === "hamlet.chief",
    );
    const data = chief?.data as { task: string };
    expect(data.task.startsWith(task)).toBe(true);
    expect(
        (await home.harness.snapshot(TaskDoc, data.task, context))?.messages,
    ).toBe(4);
});

it("refuses missing roads and records a mayor's refusal of a requested road", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, "home", "scout");
    await addAgent(home, "home", "smith");
    fauxAgents(faux, {
        "home:1": [
            tool("ask_agent", { name: "smith", message: "hi" }),
            tool("request_road", { to: "smith", reason: "Work" }),
            fauxAssistantMessage("Done"),
        ],
        "home:0": [fauxAssistantMessage("Not needed")],
    });
    const sub = await submitLine(
        home,
        { agent: scout, chief: home.chief },
        { id: "road", text: "Ask" },
        context,
    );
    await sub.wait(context);
    expect(
        (await home.harness.snapshot(RecordDoc, "home", context))?.decisions,
    ).toContainEqual(
        expect.objectContaining({
            kind: "road_refused",
            from: scout.number,
            to: "smith",
            reason: "Not needed",
        }),
    );
});

it("charges a question across hamlets to its sender", async () => {
    const { ledgerOf } = await import("../src/ledger.ts");
    const { localDay } = await import("../src/time.ts");
    const { models, faux } = fauxModels(["faux-1"], 10);
    const home = await testHome(scratch(), { models });
    await addHamlet(home, "mill", "miller");
    fauxAgents(faux, {
        "home:0": [
            tool("ask_agent", { name: "mayor@mill", message: "Ready?" }),
            fauxAssistantMessage("Done"),
        ],
        "mill:0": [fauxAssistantMessage("Ready")],
    });
    await (await say(home, "Ask mill")).wait(context);
    expect(await ledgerOf(home, "mill", localDay(home.now()), context)).toEqual(
        [],
    );
    expect(
        await ledgerOf(home, "home", localDay(home.now()), context),
    ).toContainEqual(expect.objectContaining({ what: "model", of: "mill" }));
});

it("returns an answered letter once after a crash before its reply is admitted", async () => {
    const { crashing } = await import("./support.ts");
    const dir = scratch();
    const { models, faux } = fauxModels();
    fauxAgents(faux, {
        "home:0": [
            tool("tell_agent", { name: "scout", message: "Check" }),
            fauxAssistantMessage("Sent"),
        ],
        "home:1": [fauxAssistantMessage("checked")],
    });
    const before = await testHome(dir, {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
        openStorage: crashing((writes) =>
            writes.some(
                (w) =>
                    w.type === "submission" &&
                    w.value.requestId?.startsWith("reply:"),
            ),
        ),
    });
    await addAgent(before, "home", "scout");
    await say(before, "Tell");
    expect(await before.harness.closed).toMatchObject({ reason: "failed" });
    fauxAgents(faux, { "home:0": [fauxAssistantMessage("Reply received")] });
    const after = await testHome(dir, {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    await vi.waitFor(async () => {
        const replies = (await transcript(await mayorChat(after))).filter(
            (e) => e.kind === "hamlet.reply",
        );
        expect(replies).toHaveLength(1);
        expect(JSON.stringify(replies)).toContain("checked");
    });
});

it("refuses a departed agent with its farewell, and a resident without a road", async () => {
    const { chatWith } = await import("./support.ts");
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, "home", "scout");
    await addAgent(home, "home", "smith");
    await home.harness.commit(async (tx) => {
        const r = await tx.doc(RecordDoc, "home", "home");
        r.archive.push({
            id: "gone",
            agent: {
                number: 3,
                name: "cook",
                purpose: "Cooked",
                model: null,
                since: 0,
            },
            how: "removed",
            into: null,
            at: home.now(),
            by: "mayor",
            reason: "No work",
            roads: [],
            merge: null,
            back: null,
        });
    }, context);
    fauxAgents(faux, {
        "home:1": [
            tool("ask_agent", { name: "cook", message: "Hi" }),
            tool("ask_agent", { name: "smith", message: "Hi" }),
            fauxAssistantMessage("Done"),
        ],
    });
    await (
        await submitLine(
            home,
            { agent: scout, chief: home.chief },
            { id: "departed", text: "Ask" },
            context,
        )
    ).wait(context);
    const entries = JSON.stringify(
        await transcript(await chatWith(home, scout)),
    );
    expect(entries).toContain("No work");
    expect(entries).toContain("was removed from the hamlet");
    expect(entries).toContain("you have no road to `smith`");
});
