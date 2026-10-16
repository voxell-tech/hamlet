import { createProvider } from "@earendil-works/pi-ai/models";
import { fauxAssistantMessage } from "@earendil-works/pi-ai/providers/faux";
import { describe, expect, it, vi } from "vitest";
import { reflect, TaskDoc } from "../src/chats.ts";
import type { ServerMessage } from "../src/protocol.ts";
import { watchChat } from "../src/transcript.ts";
import {
    admitted,
    CHIEF,
    closeAfter,
    context,
    fauxModels,
    MAYOR,
    mayorChat,
    openWaits,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

describe("answers", () => {
    it("fails an empty or thinking-only answer with C29's words and raises a fault", async () => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        faux.setResponses([
            fauxAssistantMessage([
                { type: "thinking", thinking: "Still thinking." },
            ]),
        ]);
        expect(await (await say(home, "Answer.")).wait(context)).toMatchObject({
            status: "unanswered",
        });
        await home.follow.settled(context);
        expect(await openWaits(home)).toMatchObject([
            {
                kind: "fault",
                about: "the model answered with nothing. Send the message again",
            },
        ]);
        expect(
            JSON.stringify(await transcript(await mayorChat(home))),
        ).toContain("the model answered with nothing. Send the message again");
    });

    it.each([false, true])(
        "summarises a cut-off reply and retries the same task once (second cutoff=%s)",
        async (twice) => {
            const { models, faux } = fauxModels();
            const home = await testHome(scratch(), {
                models,
                env: { HAMLET_REFLECT_EVERY: "0" },
            });
            const cut = fauxAssistantMessage("", { stopReason: "length" });
            Object.assign(cut.usage, {
                input: 30000,
                output: 20,
                totalTokens: 30020,
            });
            faux.setResponses([
                fauxAssistantMessage("x".repeat(100_000)),
                fauxAssistantMessage("An earlier answer."),
                cut,
                fauxAssistantMessage("Summary."),
                twice ? cut : fauxAssistantMessage("There is room now."),
            ]);
            await (await say(home, "Long answer.")).wait(context);
            await (await say(home, "Earlier.")).wait(context);
            const input = await say(home, "Keep this\n  word for word.");
            expect(await input.wait(context)).toMatchObject({
                status: "unanswered",
            });
            await vi.waitFor(() => expect(faux.state.callCount).toBe(5));
            const conversation = await mayorChat(home);
            await conversation.waitForIdle(context);
            await home.follow.settled(context);
            const entries = await transcript(conversation);
            const chief = entries
                .filter((entry) => entry.kind === "hamlet.chief")
                .at(-1);
            if (chief === undefined) throw new Error("missing chief entry");
            const retry = entries.find(
                (entry) => entry.kind === "hamlet.retry",
            );
            expect(retry?.data).toMatchObject({
                task: (chief.data as { task: string }).task,
            });
            expect(
                (await conversation.context(context)).messages,
            ).toContainEqual(
                expect.objectContaining({
                    role: "user",
                    content: "Keep this\n  word for word.",
                }),
            );
            expect(
                entries.filter((entry) => entry.kind === "pi.compaction"),
            ).toHaveLength(1);
            expect(
                (
                    await home.harness.snapshot(
                        TaskDoc,
                        (chief.data as { task: string }).task,
                        context,
                    )
                )?.spent.tokens,
            ).toBeGreaterThanOrEqual(30020);
            if (twice)
                expect(await openWaits(home)).toMatchObject([
                    {
                        kind: "fault",
                        about: "the model has no room left to answer, even with the conversation summarised. Start a new session, or choose a model that holds more",
                    },
                ]);
            else
                expect(
                    (await conversation.context(context)).messages.at(-1),
                ).toMatchObject({
                    role: "assistant",
                    content: [{ type: "text", text: "There is room now." }],
                });
        },
    );
});

it("summarises at the selected 80% threshold and keeps the unanswered message exactly", async () => {
    const { models, faux } = fauxModels();
    models.setProvider(
        createProvider({
            id: "faux",
            auth: {
                apiKey: { name: "Faux", resolve: async () => ({ auth: {} }) },
            },
            models: [{ ...faux.getModel(), contextWindow: 32768 }],
            api: { stream: faux.stream, streamSimple: faux.streamSimple },
        }),
    );
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    expect(home.compaction.reserveTokens).toBe(Math.round(32768 * 0.2));
    let summaries = 0;
    const shown: ServerMessage[] = [];
    const watch = await watchChat(
        home,
        { with: { hamlet: "home", agent: "mayor" } },
        (outcome) => {
            if (!outcome.ok) throw new Error("Chat refused");
            shown.push(outcome.value);
        },
        undefined,
        context,
    );
    if (!watch.ok) throw new Error("Chat refused");
    closeAfter(() => watch.value.stop());
    faux.setResponses(
        Array.from({ length: 20 }, () => (request) => {
            if (
                request.messages.some(
                    (message) =>
                        message.role === "system" &&
                        typeof message.content === "string" &&
                        message.content.startsWith(
                            "You are a context summarization assistant",
                        ),
                )
            ) {
                summaries++;
                return fauxAssistantMessage("Summary of the earlier turns.");
            }
            return fauxAssistantMessage("x".repeat(12000));
        }),
    );
    for (let i = 1; i <= 10; i++)
        expect(
            await (
                await say(
                    home,
                    `line ${i}
  spaces.`,
                )
            ).wait(context),
        ).toMatchObject({ status: "done" });
    expect(summaries).toBeGreaterThan(0);
    const conversation = await mayorChat(home);
    expect((await conversation.context(context)).messages).toContainEqual(
        expect.objectContaining({
            role: "user",
            content: "line 10\n  spaces.",
        }),
    );
    await vi.waitFor(() =>
        expect(
            shown
                .flatMap((message) =>
                    message.type === "transcript"
                        ? message.items
                        : message.type === "turn"
                          ? message.ops.flatMap((op) =>
                                op.op === "add" ? [op.item] : [],
                            )
                          : [],
                )
                .some(
                    (item) =>
                        item.kind === "notice" &&
                        item.text ===
                            "The conversation grew longer than the model can hold, so the earlier part of it was summarised.",
                ),
        ).toBe(true),
    );
});

it.each([false, true])(
    "uses C28's failure words when no summary can be made (history=%s)",
    async (history) => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        const cut = fauxAssistantMessage("", { stopReason: "length" });
        faux.setResponses(
            history
                ? [
                      fauxAssistantMessage("x".repeat(100000)),
                      fauxAssistantMessage("Earlier."),
                      cut,
                      fauxAssistantMessage(""),
                  ]
                : [cut],
        );
        if (history) {
            await (await say(home, "Long.")).wait(context);
            await (await say(home, "Earlier.")).wait(context);
        }
        await (await say(home, "Answer.")).wait(context);
        const words = history
            ? "the conversation is longer than the model can hold, and the model could not summarise it. Start a new session, or choose a model that holds more"
            : "the message is longer than the model can hold";
        await vi.waitFor(async () =>
            expect(await openWaits(home)).toMatchObject([
                { kind: "fault", about: words },
            ]),
        );
        expect(faux.state.callCount).toBe(history ? 4 : 1);
    },
);

it("keeps a repeated model fault until the chief dismisses it", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    const failed = fauxAssistantMessage("", {
        stopReason: "error",
        errorMessage: "Bad request",
    });
    faux.setResponses([failed, failed]);
    await (await say(home, "Fail.")).wait(context);
    await (await say(home, "Fail again.")).wait(context);
    await home.follow.settled(context);
    expect(await openWaits(home)).toMatchObject([
        { kind: "fault", asked: 2, about: "Bad request" },
    ]);
    const [wait] = await openWaits(home);
    if (wait === undefined) throw new Error("missing fault");
    const { dismiss } = await import("../src/oversight.ts");
    await home.harness.commit(
        (tx) => dismiss(tx, wait.id, home.chief, home.now()),
        context,
    );
    expect(await openWaits(home)).toEqual([]);
});

it("charges cut-off summarization to its task even when the next chief line was queued", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    let release: (() => void) | undefined;
    const cut = fauxAssistantMessage("", { stopReason: "length" });
    const summary = fauxAssistantMessage("Summary.");
    Object.assign(summary.usage, { input: 10, output: 5, totalTokens: 15 });
    faux.setResponses([
        fauxAssistantMessage("x".repeat(100000)),
        fauxAssistantMessage("Earlier."),
        async () => {
            await new Promise<void>((resolve) => {
                release = resolve;
            });
            return cut;
        },
        summary,
        fauxAssistantMessage("Queued answer."),
        fauxAssistantMessage("Carried on."),
    ]);
    await (await say(home, "Long.")).wait(context);
    await (await say(home, "Earlier.")).wait(context);
    const first = await say(home, "Cut off.");
    await vi.waitFor(() => expect(release).toBeTypeOf("function"));
    const queued = await say(home, "Queued.");
    release?.();
    await first.wait(context);
    await queued.wait(context);
    await vi.waitFor(() => expect(faux.state.callCount).toBe(6));
    await (await mayorChat(home)).waitForIdle(context);
    const entries = await transcript(await mayorChat(home));
    const input = entries.find(
        (entry) =>
            entry.kind === "hamlet.chief" &&
            (entry.data as { text: string }).text === "Cut off.",
    );
    if (input === undefined) throw new Error("missing original input");
    const { ledgerOf } = await import("../src/spend.ts");
    const { localDay } = await import("../src/home/clock.ts");
    const lines = await ledgerOf(home, "home", localDay(home.now()), context);
    expect(lines.filter((line) => line.what === "summary")).toMatchObject([
        {
            task: (input.data as { task: string }).task,
            tokens: expect.any(Number),
        },
    ]);
});

it("treats a cut-off reflection as an empty answer, without summarising or retrying it", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    faux.setResponses([
        fauxAssistantMessage("x".repeat(100000)),
        fauxAssistantMessage("Earlier."),
        fauxAssistantMessage("", { stopReason: "length" }),
        fauxAssistantMessage("Unwanted summary."),
        fauxAssistantMessage("Unwanted retry."),
    ]);
    await (await say(home, "Long.")).wait(context);
    await (await say(home, "Earlier.")).wait(context);
    const reflection = await admitted((submitted) =>
        reflect(home, { agent: MAYOR, chief: CHIEF }, context, submitted),
    );
    expect(await reflection?.wait(context)).toMatchObject({
        status: "unanswered",
    });
    await home.follow.settled(context);
    expect(JSON.stringify(await transcript(await mayorChat(home)))).toContain(
        "the model answered with nothing. Send the message again",
    );
    expect(faux.state.callCount).toBe(3);
    expect(
        (await transcript(await mayorChat(home))).filter(
            (entry) =>
                entry.kind === "pi.compaction" || entry.kind === "hamlet.retry",
        ),
    ).toEqual([]);
});
