import { randomUUID } from "node:crypto";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it } from "vitest";
import { localDay } from "../src/home/clock.ts";
import { LimitsDoc } from "../src/limits.ts";
import { RecordDoc } from "../src/record.ts";
import { ledgerOf } from "../src/spend.ts";
import { chat, mayor, request, served } from "./connection-support.ts";
import {
    addAgent,
    context,
    fauxModels,
    mayorChat,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

it("builds and takes up a chief road without refunding it, but keeps office roads", async () => {
    const { models } = fauxModels();
    const { home, connection, messages } = await served({ models });
    await addAgent(home, "home", "scout");
    await addAgent(home, "home", "smith");
    const deed = (_id: string, text: string) =>
        request(connection, messages, {
            type: "say",
            id: randomUUID(),
            to: mayor,
            text,
        });
    expect(await deed("build", "/road scout smith share")).toMatchObject({
        kind: "notice",
    });
    expect(await deed("again", "/road scout smith")).toMatchObject({
        text: expect.stringContaining("already have a road"),
    });
    expect(await deed("office", "/unroad mayor scout")).toMatchObject({
        text: expect.stringContaining("stays for as long"),
    });
    expect(
        await deed("take", "/unroad scout smith no longer needed"),
    ).toMatchObject({ kind: "notice" });
    expect(
        (await home.harness.snapshot(RecordDoc, "home", context))?.roads,
    ).toEqual([]);
    expect(
        (await ledgerOf(home, "home", localDay(home.now()), context)).filter(
            (l) => l.what === "road",
        ),
    ).toMatchObject([{ task: null, micros: 10000 }]);
    connection.send({ type: "open", id: randomUUID(), chat });
});

it("refuses a road that is unaffordable, at the limit, or has a repeated end", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    await addAgent(home, "home", "scout");
    await addAgent(home, "home", "smith");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).budget.cost = 0.005;
    }, context);
    const call = (a: string, b: string) =>
        fauxAssistantMessage(
            fauxToolCall("build_road", { a, b, reason: "Needed" }),
            { stopReason: "toolUse" },
        );
    faux.setResponses([
        call("scout", "smith"),
        call("scout", "scout"),
        call("mayor", "smith"),
        fauxAssistantMessage("Done"),
    ]);
    await (await say(home, "Build")).wait(context);
    const text = JSON.stringify(await transcript(await mayorChat(home)));
    expect(text).toContain("cannot pay for it today");
    expect(text).toContain("two different agents");
    expect(text).toContain("every agent already has a road");
});

it("checks simultaneous chief roads against one budget, in their charge transaction", async () => {
    const { road } = await import("../src/agents.ts");
    const { models } = fauxModels();
    const home = await testHome(scratch(), { models });
    for (const name of ["scout", "smith", "cook"])
        await addAgent(home, "home", name);
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).budget.cost = 0.015;
    }, context);
    const results = await Promise.allSettled(
        ["scout smith", "scout cook"].map((rest) =>
            road(home, { hamlet: "home", chief: home.chief, rest }, context),
        ),
    );
    expect(results.filter((r) => r.status === "fulfilled")).toHaveLength(1);
    expect(
        (await home.harness.snapshot(RecordDoc, "home", context))?.roads,
    ).toHaveLength(1);
});
