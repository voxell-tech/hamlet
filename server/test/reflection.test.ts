import { existsSync } from "node:fs";
import { join } from "node:path";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { describe, expect, it, vi } from "vitest";
import { ReflectionDoc } from "../src/reflection.ts";
import { SettingsDoc } from "../src/settings.ts";
import {
    followHome,
    newChat,
    reflect,
    resumeChat,
    rewind,
    stopAgent,
} from "../src/talk.ts";
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

describe("reflection", () => {
    it("counts successful turns, submits a real reflection every N, and honors the environment override", async () => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: { HAMLET_REFLECT_EVERY: "2" },
        });
        await home.harness.commit(async (tx) => {
            (await tx.doc(SettingsDoc)).reflectEvery = 1;
        }, context);
        faux.setResponses([
            fauxAssistantMessage("One."),
            fauxAssistantMessage("Two."),
            (request) => {
                expect(request.messages.at(-1)).toMatchObject({
                    role: "user",
                    content: expect.stringContaining("Update NOTES.md"),
                });
                return fauxAssistantMessage("Noted.");
            },
        ]);
        await (await say(home, "First.")).wait(context);
        await followHome(home, context);
        expect(faux.state.callCount).toBe(1);
        await (await say(home, "Second.")).wait(context);
        await vi.waitFor(async () =>
            expect(
                (await transcript(await mayorChat(home))).filter(
                    (entry) => entry.kind === "hamlet.reflection",
                ),
            ).toHaveLength(1),
        );
        await (await mayorChat(home)).waitForIdle(context);
        await followHome(home, context);
        expect(
            await home.harness.snapshot(
                ReflectionDoc,
                (await mayorChat(home)).id,
                context,
            ),
        ).toEqual({ unreflected: 0 });
    });

    it("allows explicit reflection, does not reflect failed turns, and treats zero as never", async () => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        faux.setResponses([
            fauxAssistantMessage("", {
                stopReason: "error",
                errorMessage: "Bad request",
            }),
            fauxAssistantMessage("Yes."),
            fauxAssistantMessage("Noted."),
        ]);
        await (await say(home, "Fail.")).wait(context);
        await followHome(home, context);
        expect(
            await home.harness.snapshot(
                ReflectionDoc,
                (await mayorChat(home)).id,
                context,
            ),
        ).toEqual({ unreflected: 0 });
        await (await say(home, "Succeed.")).wait(context);
        const reflection = await reflect(home, chat, context);
        expect(await reflection?.wait(context)).toMatchObject({
            status: "done",
        });
        await followHome(home, context);
        expect(faux.state.callCount).toBe(3);
        await home.close(context);
        expect(faux.state.callCount).toBe(3);
    });

    it("reflects owed turns before new and at orderly home close, keeping the ordinal across reopen", async () => {
        const dir = scratch();
        const { models, faux } = fauxModels();
        faux.setResponses([
            fauxAssistantMessage("Old."),
            fauxAssistantMessage("Old noted."),
            fauxAssistantMessage("New."),
            fauxAssistantMessage("New noted."),
        ]);
        const home = await testHome(dir, { models, env: {} });
        await (await say(home, "Old chief line.")).wait(context);
        const old = await mayorChat(home);
        const current = await newChat(home, chat, context);
        expect(current.id).not.toBe(old.id);
        await old.waitForIdle(context);
        expect(
            (await transcript(old)).map((entry) => entry.data),
        ).toContainEqual(
            expect.objectContaining({
                text: "Putting this session away. The agent keeps what is worth keeping first.",
            }),
        );
        await (await say(home, "New chief line.")).wait(context);
        await home.close(context);
        const reopened = await testHome(dir, { models, env: {} });
        const entries = await transcript(await mayorChat(reopened));
        expect(
            entries.filter((entry) => entry.kind === "hamlet.reflection"),
        ).toHaveLength(1);
        expect(faux.state.callCount).toBe(4);
        const { ChatDoc } = await import("../src/chats.ts");
        const { agentKey } = await import("../src/hamlet.ts");
        const key = agentKey(MAYOR);
        expect(
            (await reopened.harness.snapshot(ChatDoc, key, context))?.chats[
                CHIEF
            ]?.reflections,
        ).toBe(2);
    });
});

it("uses a new ordinal for each automatic or explicit reflection, even at the same count", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models, env: {} });
    await home.harness.commit(async (tx) => {
        (await tx.doc(SettingsDoc)).reflectEvery = 1;
    }, context);
    faux.setResponses(
        Array.from({ length: 6 }, () => fauxAssistantMessage("Noted.")),
    );
    await (await say(home, "one")).wait(context);
    await followHome(home, context);
    await (await mayorChat(home)).waitForIdle(context);
    await (await say(home, "two")).wait(context);
    await followHome(home, context);
    await (await mayorChat(home)).waitForIdle(context);
    const explicit = await Promise.all([
        reflect(home, chat, context),
        reflect(home, chat, context),
    ]);
    await Promise.all(explicit.map((each) => each?.wait(context)));
    await followHome(home, context);
    const entries = await transcript(await mayorChat(home));
    expect(
        entries.filter((entry) => entry.kind === "hamlet.reflection"),
    ).toHaveLength(4);
    expect(faux.state.callCount).toBe(6);
});

it("bounds shutdown reflection at 120 seconds and resumes the same submission after reopening", async () => {
    const dir = scratch();
    const { models, faux } = fauxModels();
    let started: (() => void) | undefined;
    const reflecting = new Promise<void>((resolve) => {
        started = resolve;
    });
    faux.setResponses([
        fauxAssistantMessage("Answered."),
        async (_request, options) => {
            started?.();
            await new Promise<void>((_resolve, reject) => {
                const signal = options?.signal;
                if (signal === undefined)
                    throw new Error("missing model signal");
                signal.addEventListener("abort", () => reject(signal.reason), {
                    once: true,
                });
            });
            return fauxAssistantMessage("Never kept.");
        },
        fauxAssistantMessage("Noted after reopening."),
    ]);
    const home = await testHome(dir, { models, env: {} });
    await (await say(home, "Remember.")).wait(context);
    await followHome(home, context);
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
        const closing = home.close(context);
        await reflecting;
        await vi.waitFor(() => expect(vi.getTimerCount()).toBeGreaterThan(0));
        await vi.advanceTimersByTimeAsync(120_000);
        await closing;
    } finally {
        vi.useRealTimers();
    }
    const reopened = await testHome(dir, { models, env: {} });
    await (await mayorChat(reopened)).waitForIdle(context);
    await followHome(reopened, context);
    expect(
        (await transcript(await mayorChat(reopened))).filter(
            (entry) => entry.kind === "hamlet.reflection",
        ),
    ).toHaveLength(1);
    expect(faux.state.callCount).toBe(3);
    const { ChatDoc } = await import("../src/chats.ts");
    const { agentKey } = await import("../src/hamlet.ts");
    expect(
        (await reopened.harness.snapshot(ChatDoc, agentKey(MAYOR), context))
            ?.chats[CHIEF]?.reflections,
    ).toBe(1);
});

it("resets the count when a reflection cannot start because its model is unavailable", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    faux.setResponses([fauxAssistantMessage("Answered.")]);
    await (await say(home, "Remember.")).wait(context);
    await followHome(home, context);
    models.deleteProvider("faux");
    expect(
        await (await reflect(home, chat, context))?.wait(context),
    ).toMatchObject({ status: "unanswered", reason: "no_model" });
    await followHome(home, context);
    expect(
        (
            await home.harness.snapshot(
                ReflectionDoc,
                (
                    await mayorChat(home)
                ).id,
                context,
            )
        )?.unreflected,
    ).toBe(0);
});

it("keeps shutdown prompt with a running chief turn and reflects its queued work after reopening", async () => {
    const dir = scratch();
    const { models, faux } = fauxModels();
    let running = false;
    faux.setResponses([
        fauxAssistantMessage("First answer."),
        async (_request, options) => {
            running = true;
            await new Promise<void>((_resolve, reject) => {
                const signal = options?.signal;
                if (signal === undefined)
                    throw new Error("missing model signal");
                signal.addEventListener("abort", () => reject(signal.reason), {
                    once: true,
                });
            });
            return fauxAssistantMessage("Never kept.");
        },
        fauxAssistantMessage("Carried on."),
        fauxAssistantMessage("Noted."),
    ]);
    const home = await testHome(dir, { models, env: {} });
    await (await say(home, "Remember this.")).wait(context);
    await followHome(home, context);
    const input = await say(home, "Long work.");
    await vi.waitFor(() => expect(running).toBe(true));
    const at = Date.now();
    await home.close(context);
    expect(Date.now() - at).toBeLessThan(2000);
    const reopened = await testHome(dir, { models, env: {} });
    expect(
        await (await reopened.harness.submission(input.id, context))?.wait(
            context,
        ),
    ).toMatchObject({ status: "done" });
    await (await mayorChat(reopened)).waitForIdle(context);
    await followHome(reopened, context);
    expect(
        (await transcript(await mayorChat(reopened))).filter(
            (entry) => entry.kind === "hamlet.reflection",
        ),
    ).toHaveLength(1);
    expect(
        (
            await reopened.harness.snapshot(
                ReflectionDoc,
                (
                    await mayorChat(reopened)
                ).id,
                context,
            )
        )?.unreflected,
    ).toBe(0);
    expect(faux.state.callCount).toBe(4);
});

it("withdraws a queued reflection on stop, counts only dropped chief lines, then reflects and rewinds again", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "1" },
    });
    await setFolder(home, scratch());
    let release: (() => void) | undefined;
    faux.setResponses([
        async () => {
            await new Promise<void>((resolve) => {
                release = resolve;
            });
            return fauxAssistantMessage("A answered.");
        },
        fauxAssistantMessage(fauxToolCall("bash", { command: "echo held" }), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("C answered."),
        fauxAssistantMessage("Noted."),
    ]);
    const first = await say(home, "A");
    await vi.waitFor(() => expect(release).toBeTypeOf("function"));
    const second = await say(home, "B");
    release?.();
    await first.wait(context);
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(1));
    await followHome(home, context);
    const conversation = await mayorChat(home);
    const { InboxDoc } = await import("@earendil-works/pi-durable");
    expect(
        (
            await home.harness.snapshot(InboxDoc, conversation.id, context)
        )?.items.map((item) => item.entry?.kind),
    ).toEqual(["hamlet.reflection"]);
    const dropped = await say(home, "Drop this chief line.");
    await stopAgent(home, MAYOR, context);
    expect(await second.wait(context)).toMatchObject({ status: "unanswered" });
    expect(await dropped.wait(context)).toMatchObject({ status: "unanswered" });
    expect(
        (await transcript(conversation)).map((entry) => entry.data),
    ).toContainEqual(
        expect.objectContaining({
            text: "Stopped: stopped by chief (1 queued dropped)",
        }),
    );
    await (await say(home, "C")).wait(context);
    await followHome(home, context);
    await conversation.waitForIdle(context);
    await followHome(home, context);
    expect(
        (await transcript(conversation)).filter(
            (entry) => entry.kind === "hamlet.reflection",
        ),
    ).toHaveLength(1);
    const target = (await transcript(conversation)).find(
        (entry) =>
            entry.kind === "hamlet.chief" &&
            (entry.data as { text: string }).text === "C",
    );
    expect(await rewind(home, chat, `${target?.id}:0`, context)).toBe("C");
});

it("refuses new reflection approvals at close without waiting or keeping an unanswerable wait", async () => {
    const dir = scratch();
    const folder = scratch();
    const { models, faux } = fauxModels();
    faux.setResponses([
        fauxAssistantMessage("Answered."),
        fauxAssistantMessage(
            fauxToolCall("bash", { command: "touch reflected" }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("Nothing ran."),
    ]);
    const home = await testHome(dir, {
        models,
        env: { HAMLET_REFLECT_EVERY: "10" },
    });
    await setFolder(home, folder);
    await (await say(home, "Remember.")).wait(context);
    await followHome(home, context);
    const conversation = await mayorChat(home);
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    let closed = false;
    const closing = home.close(context).then(() => {
        closed = true;
    });
    try {
        await vi.waitFor(() => expect(closed).toBe(true));
    } finally {
        await vi.advanceTimersByTimeAsync(120_000);
        await closing;
        vi.useRealTimers();
    }
    expect(existsSync(join(folder, "reflected"))).toBe(false);
    const reopened = await testHome(dir, {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    expect(await openWaits(reopened)).toEqual([]);
    expect(
        JSON.stringify(await transcript(conversation.id, reopened.harness)),
    ).toContain("nobody is here to approve it; it did not run");
    expect(faux.state.callCount).toBe(3);
});

it("withdraws the reflection owed by a retiring conversation and can reflect after resuming it", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    await setFolder(home, scratch());
    faux.setResponses([
        fauxAssistantMessage("A answered."),
        fauxAssistantMessage(fauxToolCall("bash", { command: "echo held" }), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("C answered."),
        fauxAssistantMessage("Noted."),
    ]);
    await (await say(home, "A")).wait(context);
    const old = await mayorChat(home);
    const held = await say(home, "B");
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(1));
    await newChat(home, chat, context);
    await stopAgent(home, MAYOR, context);
    expect(await held.wait(context)).toMatchObject({ status: "unanswered" });
    await followHome(home, context);
    expect(faux.state.callCount).toBe(2);
    await resumeChat(home, chat, 1, context);
    await home.harness.commit(async (tx) => {
        (await tx.doc(SettingsDoc)).reflectEvery = 1;
    }, context);
    await (await say(home, "C")).wait(context);
    await followHome(home, context);
    await old.waitForIdle(context);
    await followHome(home, context);
    expect(
        (await transcript(old)).filter(
            (entry) => entry.kind === "hamlet.reflection",
        ),
    ).toHaveLength(1);
    const target = (await transcript(old)).find(
        (entry) =>
            entry.kind === "hamlet.chief" &&
            (entry.data as { text: string }).text === "C",
    );
    expect(await rewind(home, chat, `${target?.id}:0`, context)).toBe("C");
});
