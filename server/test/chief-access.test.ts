import { randomUUID } from "node:crypto";
import { lstat, readdir } from "node:fs/promises";
import { join } from "node:path";
import { expect, it } from "vitest";
import { houseOf } from "../src/home/layout.ts";
import { RealmDoc } from "../src/realm.ts";
import { request, served } from "./connection-support.ts";
import {
    addHamlet,
    context,
    fauxModels,
    scratch,
    testHome,
} from "./support.ts";

it.each(["removed", "foreign", "erased"] as const)(
    "refuses file and skill gifts to a %s hamlet, including a gift already uploading",
    async (state) => {
        const { models, faux } = fauxModels();
        const run = await served({
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        await addHamlet(run.home, "mill", "miller");
        const to = { hamlet: "mill", agent: "mayor" };
        const send = (message: Parameters<typeof request>[2]) =>
            request(run.connection, run.messages, message);
        const begun = await send({
            type: "give",
            id: randomUUID(),
            to,
            files: [{ path: "note.txt", size: 1 }],
        });
        if (begun.type !== "reply" || !begun.upload)
            throw new Error("No upload");
        await run.home.harness.commit(async (tx) => {
            const realm = await tx.doc(RealmDoc);
            const place = realm.hamlets.find((h) => h.id === "mill");
            if (!place) throw new Error("No mill");
            if (state === "removed")
                place.removed = { at: 1, by: "chief", reason: "Finished" };
            if (state === "foreign") place.chiefs = ["another-chief"];
            if (state === "erased") place.erased = true;
        }, context);
        const reason =
            state === "removed"
                ? "was removed: Finished"
                : state === "foreign"
                  ? "Only a chief"
                  : "no hamlet named `mill`";
        for (const message of [
            {
                type: "give-part" as const,
                id: randomUUID(),
                upload: begun.upload,
                path: "note.txt",
                offset: 0,
                data: "QQ==",
            },
            {
                type: "give" as const,
                id: randomUUID(),
                to,
                files: [{ path: "other.txt", size: 0 }],
            },
            {
                type: "give" as const,
                id: randomUUID(),
                to,
                skill: { starter: "system-info" },
            },
            {
                type: "give" as const,
                id: randomUUID(),
                to: { hamlet: "home", agent: "mayor" },
                skill: { name: "system-info", from: to },
            },
        ]) {
            expect(await send(message)).toMatchObject({
                type: "reply",
                kind: "error",
                text: expect.stringContaining(reason),
            });
        }
        const house = houseOf(run.home.layout, { hamlet: "mill", number: 0 });
        if (state === "erased")
            expect(await lstat(house).catch(() => null)).toBeNull();
        else expect(await readdir(join(house, "inbox"))).toEqual([]);
        expect(await readdir(run.home.layout.gifts)).toEqual([]);
        expect(faux.state.callCount).toBe(0);
    },
);

it("refuses /to across a chief boundary before recording an instruction or starting a turn", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    await addHamlet(run.home, "mill", "miller");
    await run.home.harness.commit(async (tx) => {
        const place = (await tx.doc(RealmDoc)).hamlets.find(
            (h) => h.id === "mill",
        );
        if (!place) throw new Error("No mill");
        place.chiefs = ["another-chief"];
    }, context);
    const { ChatDoc } = await import("../src/chats.ts");
    const before = await run.home.harness.snapshot(ChatDoc, "mill:0", context);
    expect(
        await request(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: { hamlet: "home", agent: "mayor" },
            text: "/to miller Do this",
        }),
    ).toMatchObject({
        type: "reply",
        kind: "error",
        text: expect.stringContaining("Only a chief"),
    });
    expect(faux.state.callCount).toBe(0);
    expect(await run.home.harness.snapshot(ChatDoc, "mill:0", context)).toEqual(
        before,
    );
});

it.each(["removed", "foreign", "erased"] as const)(
    "refuses chief conversations and paging in a %s hamlet",
    async (state) => {
        const run = await served({
            models: fauxModels().models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        await addHamlet(run.home, "mill", "miller");
        const to = { hamlet: "mill", agent: "mayor" };
        expect(
            await request(run.connection, run.messages, {
                type: "open",
                id: randomUUID(),
                chat: { with: to },
            }),
        ).toMatchObject({ type: "transcript" });
        await run.home.harness.commit(async (tx) => {
            const place = (await tx.doc(RealmDoc)).hamlets.find(
                (h) => h.id === "mill",
            );
            if (!place) throw new Error("No mill");
            if (state === "removed")
                place.removed = { at: 1, by: "chief", reason: "Finished" };
            if (state === "foreign") place.chiefs = ["another-chief"];
            if (state === "erased") place.erased = true;
        }, context);
        const reason =
            state === "removed"
                ? "was removed: Finished"
                : state === "foreign"
                  ? "Only a chief"
                  : "no hamlet named `mill`";
        for (const operation of [
            { type: "open" as const, chat: { with: to }, before: "1" },
            { type: "open" as const, chat: { with: to, past: 1 } },
            {
                type: "open" as const,
                chat: { with: to, from: { hamlet: "home", number: 0 } },
            },
            { type: "say" as const, to, text: "Hello" },
            { type: "new" as const, to },
            { type: "retry" as const, to },
            { type: "stop" as const, agent: to },
            { type: "read" as const, what: "papers" as const, agent: to },
            {
                type: "edit" as const,
                doc: "notes" as const,
                agent: to,
                text: "Changed",
            },
            {
                type: "edit" as const,
                doc: "limits" as const,
                hamlet: "mill",
                set: { section: "budget", key: "cost", value: 8 },
            },
            { type: "resume" as const, to, number: 1 },
            { type: "rewind" as const, chat: { with: to }, item: "1" },
        ])
            expect(
                await request(run.connection, run.messages, {
                    ...operation,
                    id: randomUUID(),
                }),
            ).toMatchObject({ text: expect.stringContaining(reason) });
    },
);

it.each(["typed", "protocol"] as const)(
    "%s stop-all leaves foreign-chief agents working",
    async (path) => {
        const { fauxAssistantMessage, fauxToolCall } = await import(
            "@earendil-works/pi-ai/providers/faux"
        );
        const { submitLine } = await import("../src/chats.ts");
        const { fauxAgents, openWaits } = await import("./support.ts");
        const { vi } = await import("vitest");
        const { models, faux } = fauxModels();
        const run = await served({
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        await addHamlet(run.home, "mill", "miller");
        fauxAgents(
            faux,
            Object.fromEntries(
                ["home:0", "mill:0"].map((key) => [
                    key,
                    [
                        fauxAssistantMessage(
                            fauxToolCall("ask_chief", {
                                question: "Continue?",
                            }),
                            { stopReason: "toolUse" },
                        ),
                        fauxAssistantMessage("Done"),
                    ],
                ]),
            ),
        );
        for (const hamlet of ["home", "mill"])
            await submitLine(
                run.home,
                { agent: { hamlet, number: 0 }, chief: run.home.chief },
                { id: randomUUID(), text: "Ask me" },
                context,
            );
        await vi.waitFor(async () =>
            expect(await openWaits(run.home)).toHaveLength(2),
        );
        await run.home.harness.commit(async (tx) => {
            const place = (await tx.doc(RealmDoc)).hamlets.find(
                (h) => h.id === "mill",
            );
            if (place) place.chiefs = ["another-chief"];
        }, context);
        await request(
            run.connection,
            run.messages,
            path === "typed"
                ? {
                      type: "say",
                      id: randomUUID(),
                      to: { hamlet: "home", agent: "mayor" },
                      text: "/stop all",
                  }
                : { type: "stop", id: randomUUID() },
        );
        expect((await openWaits(run.home)).map((w) => w.agent?.hamlet)).toEqual(
            ["mill"],
        );
    },
);

it("refuses protocol and typed answers to a foreign hamlet's wait", async () => {
    const { fauxAssistantMessage, fauxToolCall } = await import(
        "@earendil-works/pi-ai/providers/faux"
    );
    const { submitLine } = await import("../src/chats.ts");
    const { openWaits } = await import("./support.ts");
    const { vi } = await import("vitest");
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    await addHamlet(run.home, "mill", "miller");
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("ask_chief", { question: "Continue?" }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("Done"),
    ]);
    await submitLine(
        run.home,
        { agent: { hamlet: "mill", number: 0 }, chief: run.home.chief },
        { id: randomUUID(), text: "Ask me" },
        context,
    );
    await vi.waitFor(async () =>
        expect(await openWaits(run.home)).toHaveLength(1),
    );
    const wait = (await openWaits(run.home))[0];
    if (!wait) throw new Error("No wait");
    await run.home.harness.commit(async (tx) => {
        const place = (await tx.doc(RealmDoc)).hamlets.find(
            (h) => h.id === "mill",
        );
        if (place) place.chiefs = ["another-chief"];
    }, context);
    for (const operation of [
        {
            type: "answer" as const,
            wait: wait.id,
            choice: "reply" as const,
            text: "Yes",
        },
        {
            type: "say" as const,
            to: { hamlet: "home", agent: "mayor" },
            text: "/reply Yes",
        },
    ])
        expect(
            await request(run.connection, run.messages, {
                ...operation,
                id: randomUUID(),
            }),
        ).toMatchObject({
            type: "reply",
            kind: "error",
            text: expect.stringContaining("Only a chief"),
        });
    expect(await openWaits(run.home)).toHaveLength(1);
});

it("finds a living received agent once, including its office address facts", async () => {
    const { findInState } = await import("../src/realm/roads.ts");
    const { stateOf } = await import("../src/state.ts");
    const home = await testHome(scratch(), { models: fauxModels().models });
    const state = await stateOf(home, { epoch: 0, revision: 0 }, context);
    expect(findInState(state, { agent: "mayor" })).toMatchObject({
        ref: { hamlet: "home", number: 0 },
        mayor: true,
        name: "founder",
    });
});
