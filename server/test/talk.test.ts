import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { describe, expect, it, vi } from "vitest";
import { answer } from "../src/gate.ts";
import { HOME } from "../src/hamlet.ts";
import { historyOf } from "../src/history.ts";
import { LimitsDoc } from "../src/limits.ts";
import { ReflectionDoc } from "../src/reflection.ts";
import { followHome, newChat, resumeChat, retry } from "../src/talk.ts";
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

describe("the chief's queue", () => {
    it("answers the next line even when the running turn failed", async () => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        let release: (() => void) | undefined;
        faux.setResponses([
            async () => {
                await new Promise<void>((resolve) => {
                    release = resolve;
                });
                return fauxAssistantMessage("", {
                    stopReason: "error",
                    errorMessage: "Bad request",
                });
            },
            fauxAssistantMessage("The queued line answers."),
        ]);
        const first = await say(home, "Fail.");
        await vi.waitFor(() => expect(release).toBeTypeOf("function"));
        const second = await say(home, "Still answer.");
        release?.();
        expect(await first.wait(context)).toMatchObject({
            status: "unanswered",
        });
        expect(await second.wait(context)).toMatchObject({ status: "done" });
        expect(faux.state.callCount).toBe(2);
    });
});

const own = { agent: MAYOR, chief: CHIEF };

it("keeps arrival order, including queued lines behind a budget stop", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    let release: (() => void) | undefined;
    const seen: string[] = [];
    faux.setResponses([
        async () => {
            await new Promise<void>((resolve) => {
                release = resolve;
            });
            return fauxAssistantMessage("First.");
        },
        (request) => {
            seen.push(
                String(
                    request.messages
                        .filter((message) => message.role === "user")
                        .at(-1)?.content,
                ),
            );
            return fauxAssistantMessage("Second.");
        },
        (request) => {
            seen.push(
                String(
                    request.messages
                        .filter((message) => message.role === "user")
                        .at(-1)?.content,
                ),
            );
            return fauxAssistantMessage("Third.");
        },
    ]);
    const first = await say(home, "one");
    await vi.waitFor(() => expect(release).toBeTypeOf("function"));
    const second = await say(home, "two");
    const third = await say(home, "three");
    release?.();
    await Promise.all([
        first.wait(context),
        second.wait(context),
        third.wait(context),
    ]);
    expect(seen).toEqual(["two", "three"]);
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, HOME, HOME)).budget.cost = 0;
    }, context);
    const stopped = await Promise.all([
        say(home, "four"),
        say(home, "five"),
        say(home, "six"),
    ]);
    expect(
        await Promise.all(stopped.map((each) => each.wait(context))),
    ).toMatchObject([
        { status: "unanswered" },
        { status: "unanswered" },
        { status: "unanswered" },
    ]);
    await followHome(home, context);
    expect(
        (await transcript(await mayorChat(home))).filter(
            (entry) =>
                entry.kind === "hamlet.notice" &&
                String((entry.data as { text: string }).text).includes(
                    "The hamlet has spent",
                ),
        ),
    ).toHaveLength(3);
    expect(faux.state.callCount).toBe(3);
});

it("carries a failed turn on under a new task, without another chief line, and refuses after an answer", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    faux.setResponses([
        fauxAssistantMessage(fauxToolCall("read", { path: "NOTES.md" }), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("", {
            stopReason: "error",
            errorMessage: "Bad request",
        }),
        fauxAssistantMessage("Carried on."),
    ]);
    await (await say(home, "Read.")).wait(context);
    expect(await (await retry(home, own, context)).wait(context)).toMatchObject(
        { status: "done" },
    );
    const entries = await transcript(await mayorChat(home));
    const chief = entries.find((entry) => entry.kind === "hamlet.chief");
    const carried = entries.find((entry) => entry.kind === "hamlet.retry");
    expect(
        entries.filter((entry) => entry.kind === "hamlet.chief"),
    ).toHaveLength(1);
    expect(carried?.data).toMatchObject({ task: expect.any(String) });
    if (carried === undefined || chief === undefined)
        throw new Error("missing turn entries");
    expect((carried.data as { task: string }).task).not.toBe(
        (chief.data as { task: string }).task,
    );
    await expect(retry(home, own, context)).rejects.toThrow(
        "the last message was answered",
    );
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
    ).toBe(1);
});

it("moves new immediately, keeps concurrent lines, resumes by number, and lists the chief's history", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    let release: (() => void) | undefined;
    faux.setResponses([
        async () => {
            await new Promise<void>((resolve) => {
                release = resolve;
            });
            return fauxAssistantMessage("Old answer.");
        },
        fauxAssistantMessage("New answer."),
    ]);
    const title = "x".repeat(91);
    const oldInput = await say(
        home,
        `
${title}
Ignored in the title.`,
    );
    await vi.waitFor(() => expect(release).toBeTypeOf("function"));
    const old = await mayorChat(home);
    const fresh = await newChat(home, own, context);
    const during = await say(home, "A new line.");
    expect((await during.status(context)).conversationId).toBe(fresh.id);
    release?.();
    await Promise.all([oldInput.wait(context), during.wait(context)]);
    expect(await historyOf(home, own, context)).toMatchObject([
        {
            number: 2,
            current: true,
            title: "A new line.",
            by: CHIEF,
            messages: 2,
        },
        {
            number: 1,
            current: false,
            title: `${"x".repeat(90)}...`,
            by: CHIEF,
            messages: 2,
        },
    ]);
    expect((await resumeChat(home, own, 1, context))?.id).toBe(old.id);
    expect((await mayorChat(home)).id).toBe(old.id);
    await expect(
        resumeChat(home, { ...own, chief: "bea" }, 2, context),
    ).rejects.toThrow("There is no past session");
    await newChat(home, own, context);
    expect(
        (await historyOf(home, own, context)).map((each) => each.number),
    ).toEqual([2, 1]);
});

it("asks again after resuming a conversation whose commands the chief trusted", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    await setFolder(home, scratch());
    const command = fauxAssistantMessage(
        fauxToolCall("bash", { command: "echo here" }),
        { stopReason: "toolUse" },
    );
    faux.setResponses([
        command,
        fauxAssistantMessage("Done."),
        command,
        fauxAssistantMessage("Done again."),
    ]);
    const first = await say(home, "Run.");
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(1));
    const [wait] = await openWaits(home);
    if (wait === undefined) throw new Error("missing approval");
    await answer(
        home,
        { wait: wait.id, choice: "always", chief: CHIEF },
        context,
    );
    await first.wait(context);
    await newChat(home, own, context);
    await resumeChat(home, own, 1, context);
    const resumed = await say(home, "Run again.");
    await vi.waitFor(async () =>
        expect(await openWaits(home)).toMatchObject([
            { danger: "command", chat: { chat: 1 } },
        ]),
    );
    const [again] = await openWaits(home);
    if (again === undefined) throw new Error("missing resumed approval");
    await answer(
        home,
        { wait: again.id, choice: "deny", chief: CHIEF },
        context,
    );
    expect(await resumed.wait(context)).toMatchObject({ status: "done" });
});

it("refuses retry during a running turn instead of carrying on after its answer", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    let release: (() => void) | undefined;
    faux.setResponses([
        async () => {
            await new Promise<void>((resolve) => {
                release = resolve;
            });
            return fauxAssistantMessage("Answered.");
        },
        fauxAssistantMessage("Unwanted carry-on."),
    ]);
    const first = await say(home, "Work.");
    await vi.waitFor(() => expect(release).toBeTypeOf("function"));
    const attempted = await retry(home, own, context).then(
        () => "admitted",
        (error: unknown) => String(error),
    );
    release?.();
    await first.wait(context);
    await (await mayorChat(home)).waitForIdle(context);
    expect(attempted).toBe(
        "Error: A turn is running here. Stop it, then retry.",
    );
    expect(faux.state.callCount).toBe(1);
    expect(
        (await transcript(await mayorChat(home))).some(
            (entry) => entry.kind === "hamlet.retry",
        ),
    ).toBe(false);
});
