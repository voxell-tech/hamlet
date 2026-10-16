import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { describe, expect, it, vi } from "vitest";
import {
    followHome,
    historyOf,
    newChat,
    ReflectionDoc,
    retry,
    rewind,
    usageOf,
} from "../src/chats.ts";
import { answer } from "../src/gate.ts";
import { raiseFault } from "../src/oversight.ts";
import {
    CHIEF,
    context,
    fauxModels,
    MAYOR,
    mayorChat,
    openWaits,
    say,
    scratch,
    setFolder,
    testHome,
    transcript,
} from "./support.ts";

const chat = { agent: MAYOR, chief: CHIEF };

describe("rewind", () => {
    it("forks before the chief line, preserves abandoned spend and waits, and takes the reflection count back", async () => {
        const dir = scratch();
        const { models, faux } = fauxModels();
        faux.setResponses([
            fauxAssistantMessage("1"),
            fauxAssistantMessage("2"),
            fauxAssistantMessage("3"),
        ]);
        const home = await testHome(dir, {
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        await (await say(home, "one")).wait(context);
        await (await say(home, "two")).wait(context);
        await followHome(home, context);
        const old = await mayorChat(home);
        const before = await transcript(old);
        const target = before.filter(
            (entry) => entry.kind === "hamlet.chief",
        )[1];
        if (target === undefined) throw new Error("missing target");
        await home.harness.commit(
            (tx) =>
                raiseFault(
                    tx,
                    {
                        by: "founder",
                        agent: MAYOR,
                        chat: { ...MAYOR, chief: CHIEF, chat: 1 },
                        about: "a fault after two",
                    },
                    (target.data as { at: number }).at + 1,
                ),
            context,
        );
        const paid = await usageOf(home, old.id, context);
        expect(await rewind(home, chat, `${target.id}:0`, context)).toBe("two");
        const fork = await mayorChat(home);
        expect(fork.id).not.toBe(old.id);
        expect(await openWaits(home)).toEqual([]);
        expect(
            (await fork.context(context)).messages
                .filter((message) => message.role !== "system")
                .map((message) => message.content),
        ).toEqual(["one", [{ type: "text", text: "1" }]]);
        expect(
            (await home.harness.snapshot(ReflectionDoc, fork.id, context))
                ?.unreflected,
        ).toBe(1);
        expect((await usageOf(home, fork.id, context)).calls).toBe(paid.calls);
        await expect(retry(home, chat, context)).rejects.toThrow(
            "the last message was answered",
        );
        await (await say(home, "three")).wait(context);
        const after = await transcript(fork);
        expect(
            after
                .filter((entry) => entry.conversationId === fork.id)
                .every((entry) => entry.id > (before.at(-1)?.id ?? 0)),
        ).toBe(true);
        expect(
            (await historyOf(home, chat, context))[0]?.tokens,
        ).toBeGreaterThanOrEqual(paid.input + paid.output);
        expect(await transcript(old)).toEqual(before);
        await home.close(context);
        const reopened = await testHome(dir, {
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        expect(
            (await (await mayorChat(reopened)).context(context)).messages
                .filter((message) => message.role !== "system")
                .map((message) => message.content),
        ).toEqual([
            "one",
            [{ type: "text", text: "1" }],
            "three",
            [{ type: "text", text: "3" }],
        ]);
    });

    it("refuses a running or queued turn, rejects non-chief entries, and can rewind past an earlier fork to the first line", async () => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        let release: (() => void) | undefined;
        faux.setResponses([
            fauxAssistantMessage("1"),
            async () => {
                await new Promise<void>((resolve) => {
                    release = resolve;
                });
                return fauxAssistantMessage("2");
            },
            fauxAssistantMessage("3"),
        ]);
        await (await say(home, "one")).wait(context);
        const first = (await transcript(await mayorChat(home))).find(
            (entry) => entry.kind === "hamlet.chief",
        );
        if (first === undefined) throw new Error("missing first line");
        const second = await say(home, "two");
        await vi.waitFor(() => expect(release).toBeTypeOf("function"));
        await say(home, "three");
        await expect(
            rewind(home, chat, `${first.id}:0`, context),
        ).rejects.toThrow("A turn is running here. Stop it, then rewind.");
        release?.();
        await second.wait(context);
        await (await mayorChat(home)).waitForIdle(context);
        const entries = await transcript(await mayorChat(home));
        const chief = entries.filter((entry) => entry.kind === "hamlet.chief");
        const assistant = entries.find(
            (entry) => entry.kind === "pi.assistant",
        );
        await expect(
            rewind(home, chat, `${assistant?.id}:0`, context),
        ).rejects.toThrow("chief's message");
        await rewind(home, chat, `${chief[2]?.id}:0`, context);
        await rewind(home, chat, `${first.id}:0`, context);
        expect(
            (await (await mayorChat(home)).context(context)).messages,
        ).toEqual([]);
    });
});

it("rewinds only waits from that conversation while a past conversation finishes", async () => {
    const { models, faux } = fauxModels();
    let at = 1000;
    const home = await testHome(scratch(), {
        models,
        now: () => at,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    await setFolder(home, scratch());
    let release: (() => void) | undefined;
    faux.setResponses([
        async () => {
            await new Promise<void>((resolve) => {
                release = resolve;
            });
            return fauxAssistantMessage(
                fauxToolCall("bash", { command: "echo old" }),
                { stopReason: "toolUse" },
            );
        },
        fauxAssistantMessage("New answer."),
        fauxAssistantMessage("Old answer."),
    ]);
    const oldInput = await say(home, "Old work.");
    await vi.waitFor(() => expect(release).toBeTypeOf("function"));
    const current = await newChat(home, chat, context);
    at = 2000;
    await (await say(home, "Take back only this.")).wait(context);
    const target = (await transcript(current)).find(
        (entry) => entry.kind === "hamlet.chief",
    );
    at = 3000;
    release?.();
    await vi.waitFor(async () =>
        expect(await openWaits(home)).toMatchObject([
            { chat: { chat: 1 }, about: "echo old" },
        ]),
    );
    const [wait] = await openWaits(home);
    if (wait === undefined) throw new Error("missing old approval");
    await rewind(home, chat, `${target?.id}:0`, context);
    expect(await openWaits(home)).toEqual([wait]);
    await answer(
        home,
        { wait: wait.id, choice: "allow", chief: CHIEF },
        context,
    );
    expect(await oldInput.wait(context)).toMatchObject({ status: "done" });
});

it("reflects only the kept branch at close, never the message the chief took back", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "10" },
    });
    const reflected: unknown[][] = [];
    const note = (
        request: Parameters<
            import("@earendil-works/pi-ai/providers/faux").FauxResponseFactory
        >[0],
    ) => {
        reflected.push(
            request.messages
                .filter((message) => message.role === "user")
                .map((message) => message.content),
        );
        return fauxAssistantMessage("Noted.");
    };
    faux.setResponses([
        fauxAssistantMessage("One."),
        fauxAssistantMessage("Two."),
        note,
        note,
    ]);
    await (await say(home, "Keep this.")).wait(context);
    await (await say(home, "Take this back.")).wait(context);
    const target = (await transcript(await mayorChat(home))).filter(
        (entry) => entry.kind === "hamlet.chief",
    )[1];
    await rewind(home, chat, `${target?.id}:0`, context);
    await home.close(context);
    expect(reflected).toEqual([
        ["Keep this.", expect.stringContaining("Reflection time")],
    ]);
    expect(faux.state.callCount).toBe(3);
});

it("refuses a rewind with only a queued write and no running turn", async () => {
    const { models } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    const conversation = await mayorChat(home);
    const { ChiefEntry } = await import("../src/chats.ts");
    const { InboxDoc, LiveDoc } = await import("@earendil-works/pi-durable");
    const target = await home.harness.commit(async (tx) => {
        const inbox = await tx.doc(InboxDoc, conversation.id);
        const chief = await tx.appendEntry(ChiefEntry, conversation.id, {
            data: {
                chief: CHIEF,
                text: "Take back.",
                task: "queued-only",
                at: home.now(),
            },
        });
        const queued = await tx.createSubmission({
            type: "write",
            status: "queued",
            conversationId: conversation.id,
        });
        inbox.items.push({
            id: queued.id,
            mode: "write",
            entry: {
                kind: "hamlet.notice",
                data: { text: "Queued boundary.", at: home.now() },
                edits: [],
            },
        });
        return chief.id;
    }, context);
    expect(
        (await home.harness.snapshot(LiveDoc, conversation.id, context))?.run,
    ).toBeUndefined();
    await expect(rewind(home, chat, `${target}:0`, context)).rejects.toThrow(
        "A turn is running here. Stop it, then rewind.",
    );
});
