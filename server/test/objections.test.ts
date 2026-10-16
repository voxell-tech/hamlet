import { randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { createModels } from "@earendil-works/pi-ai/models";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { QuestionEntry, startConversation, submitLine } from "../src/chats.ts";
import { answer } from "../src/gate.ts";
import { localDay } from "../src/home/clock.ts";
import { houseOf } from "../src/home/layout.ts";
import type { Home } from "../src/home.ts";
import { LimitsDoc } from "../src/limits.ts";
import type { Choice, Wait } from "../src/oversight.ts";
import { say as typed } from "../src/say.ts";
import { SettingsDoc } from "../src/settings.ts";
import { LedgerDoc, reconcile } from "../src/spend.ts";
import {
    addAgent,
    addHamlet,
    CHIEF,
    chatWith,
    context,
    crashing,
    fauxAgents,
    fauxModels,
    MAYOR,
    mayorChat,
    openWaits,
    request,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

function call(name: string, args: Record<string, string>) {
    return fauxAssistantMessage(fauxToolCall(name, args), {
        stopReason: "toolUse",
    });
}
vi.setConfig({ testTimeout: 20000 });

function eventually<T>(check: () => T): Promise<T> {
    return vi.waitFor(check, { timeout: 10000 });
}

async function waiting(home: Home): Promise<Readonly<Wait>> {
    await eventually(async () => expect(await openWaits(home)).toHaveLength(1));
    const wait = (await openWaits(home))[0];
    if (!wait) throw new Error("No wait");
    return wait;
}

function rule(home: Home, wait: Readonly<Wait>, choice: Choice, text = "") {
    return answer(home, { wait: wait.id, choice, text, chief: CHIEF }, context);
}

it("holds an objection once per instruction and goes on only when the chief says go (H10)", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    faux.setResponses([
        call("object", { reason: "The list is the only copy." }),
        call("object", { reason: "I still think so." }),
        call("write", { path: "done.txt", content: "done" }),
        fauxAssistantMessage("Done."),
    ]);
    const submission = await say(home, "Delete the reading list.");
    const wait = await waiting(home);
    expect(wait).toMatchObject({
        kind: "objection",
        danger: null,
        by: "mayor",
        answers: ["go", "withdraw"],
        about: "chief told mayor: Delete the reading list.\nmayor objects: The list is the only copy.",
    });
    expect(existsSync(join(houseOf(home.layout, MAYOR), "done.txt"))).toBe(
        false,
    );
    await rule(home, wait, "go", "I have a backup.");
    expect(await submission.wait(context)).toMatchObject({ status: "done" });
    expect(await openWaits(home)).toEqual([]);
    expect(existsSync(join(houseOf(home.layout, MAYOR), "done.txt"))).toBe(
        true,
    );
    const text = JSON.stringify(await transcript(await mayorChat(home)));
    expect(text).toContain("The chief says go ahead: I have a backup.");
    expect(text).toContain("may not object");
});

it("stops a withdrawn instruction before any further work", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    let later = 0;
    faux.setResponses([
        call("object", { reason: "The only copy." }),
        () => {
            later++;
            return call("write", { path: "done.txt", content: "done" });
        },
    ]);
    const submission = await say(home, "Delete the list.");
    await rule(home, await waiting(home), "withdraw", "You are right.");
    expect(await submission.wait(context)).toMatchObject({
        status: "unanswered",
    });
    expect(later).toBe(0);
    expect(existsSync(join(houseOf(home.layout, MAYOR), "done.txt"))).toBe(
        false,
    );
    expect(JSON.stringify(await transcript(await mayorChat(home)))).toContain(
        "withdrew the instruction: You are right.",
    );
});

it("asks three questions per turn, accepting a typed reply and a dismissal (H10)", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    faux.setResponses([
        call("ask_chief", { question: "Paper or ebook?" }),
        call("ask_chief", { question: "Hardback?" }),
        call("ask_chief", { question: "Which edition?" }),
        call("ask_chief", { question: "A fourth?" }),
        fauxAssistantMessage("Ordered."),
        call("ask_chief", { question: "Another turn?" }),
        fauxAssistantMessage("Done."),
    ]);
    const submission = await say(home, "Order Dune.");
    const first = await waiting(home);
    expect(first).toMatchObject({
        kind: "question",
        danger: null,
        about: "Paper or ebook?",
        answers: ["reply", "dismiss"],
    });
    expect(
        await typed(
            home,
            {
                type: "say",
                id: randomUUID(),
                to: { hamlet: "home", agent: "mayor" },
                text: "Paper.",
            },
            context,
        ),
    ).toMatchObject({ ok: true });
    await rule(home, await waiting(home), "dismiss");
    await rule(home, await waiting(home), "reply", "First edition.");
    expect(await submission.wait(context)).toMatchObject({ status: "done" });
    expect(await openWaits(home)).toEqual([]);
    const text = JSON.stringify(await transcript(await mayorChat(home)));
    expect(text).toContain("The chief answers: Paper.");
    expect(text).toContain("The chief did not answer");
    expect(text).toContain("asked the chief 3 questions this turn");
    const next = await say(home, "Order another.");
    await rule(home, await waiting(home), "reply", "Yes.");
    expect(await next.wait(context)).toMatchObject({ status: "done" });
});

it("restores the same unanswered objection after reopening the home (H34)", async () => {
    const { models, faux } = fauxModels();
    const dir = scratch();
    const before = await testHome(dir, {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    faux.setResponses([
        call("object", { reason: "The only copy." }),
        fauxAssistantMessage("Done."),
    ]);
    const submission = await say(before, "Delete it.");
    const held = await waiting(before);
    await before.close(context);
    const after = await testHome(dir, {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    expect(await waiting(after)).toMatchObject({ id: held.id, asked: 1 });
    await rule(after, held, "go");
    expect(
        await (await after.harness.submission(submission.id, context))?.wait(
            context,
        ),
    ).toMatchObject({ status: "done" });
    expect(await openWaits(after)).toEqual([]);
});

it("keeps a go ahead across a crash before the objection's tool result commits", async () => {
    const { models, faux } = fauxModels();
    const dir = scratch();
    const before = await testHome(dir, {
        models,
        openStorage: crashing((writes) =>
            writes.some(
                (w) => w.type === "entry" && w.value.kind === "pi.tool-result",
            ),
        ),
    });
    faux.setResponses([
        call("object", { reason: "The only copy." }),
        fauxAssistantMessage("Done."),
    ]);
    const submission = await say(before, "Delete it.");
    await rule(before, await waiting(before), "go", "A backup exists.");
    expect(await before.harness.closed).toMatchObject({ reason: "failed" });
    const after = await testHome(dir, { models });
    expect(
        await (await after.harness.submission(submission.id, context))?.wait(
            context,
        ),
    ).toMatchObject({ status: "done" });
    const text = JSON.stringify(await transcript(await mayorChat(after)));
    expect(text).toContain("The chief says go ahead: A backup exists.");
    expect(text).not.toContain("nobody is here to approve");
});

it("lets the mayor review beside the work and holds only the next step when it objects", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, "home", "scout");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).asking.mayorReview = "always";
    }, context);
    const house = houseOf(home.layout, scout);
    let nextRequested = false;
    fauxAgents(faux, {
        "home:1": [
            call("write", { path: "a.txt", content: "one" }),
            async () => {
                await waiting(home);
                nextRequested = true;
                return call("write", { path: "b.txt", content: "two" });
            },
            fauxAssistantMessage("Done."),
        ],
        errands: [
            async (prompt, _options, _state, model) => {
                const seen = request(prompt.messages, model);
                expect(seen.tools).toEqual([]);
                expect(seen.sections.instructions).toContain(
                    "Most instructions are fine.",
                );
                expect(seen.sections.instructions).toContain("scout");
                await eventually(() =>
                    expect(existsSync(join(house, "a.txt"))).toBe(true),
                );
                return fauxAssistantMessage(
                    "OBJECT: The scout needs that list.",
                );
            },
        ],
    });
    const submission = await submitLine(
        home,
        { agent: scout, chief: CHIEF },
        { id: randomUUID(), text: "Delete the list." },
        context,
    );
    const wait = await waiting(home);
    expect(wait).toMatchObject({
        kind: "objection",
        by: "mayor",
        answers: ["go", "withdraw"],
    });
    expect(wait.about).toContain(
        "The mayor objects: The scout needs that list.",
    );
    expect(existsSync(join(house, "a.txt"))).toBe(true);
    expect(existsSync(join(house, "b.txt"))).toBe(false);
    await eventually(() => expect(nextRequested).toBe(true));
    await rule(home, wait, "go");
    expect(await submission.wait(context)).toMatchObject({ status: "done" });
    expect(existsSync(join(house, "b.txt"))).toBe(true);
});

it("charges a passing review to the instruction's task without holding the work", async () => {
    const { models, faux } = fauxModels(["faux-1"], 1);
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, "home", "scout");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).asking.mayorReview = "always";
    }, context);
    fauxAgents(faux, {
        "home:1": [fauxAssistantMessage("Done.")],
        errands: [fauxAssistantMessage("pass")],
    });
    const submission = await submitLine(
        home,
        { agent: scout, chief: CHIEF },
        { id: randomUUID(), text: "Read it." },
        context,
    );
    expect(await submission.wait(context)).toMatchObject({ status: "done" });
    await eventually(async () => {
        await reconcile(home, context);
        const lines = (
            await home.harness.snapshot(
                LedgerDoc,
                `home/${localDay(home.now())}`,
                context,
            )
        )?.lines;
        expect(lines).toHaveLength(2);
        expect(lines).toContainEqual(
            expect.objectContaining({
                agent: 0,
                what: "model",
                task: expect.any(String),
            }),
        );
        const charges = lines?.filter((line) => line.what !== "reset");
        expect(
            charges?.every(
                (line) => line.task === charges[0]?.task && line.micros > 0,
            ),
        ).toBe(true);
    });
    expect(await openWaits(home)).toEqual([]);
});

it("buys no review when the task has spent its cap", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, "home", "scout");
    await home.harness.commit(async (tx) => {
        const limits = await tx.doc(LimitsDoc, "home", "home");
        limits.asking.mayorReview = "always";
        limits.task.cost = 0;
    }, context);
    let requests = 0;
    faux.setResponses([
        () => {
            requests++;
            return fauxAssistantMessage("pass");
        },
    ]);
    const submission = await submitLine(
        home,
        { agent: scout, chief: CHIEF },
        { id: randomUUID(), text: "Read it." },
        context,
    );
    expect(await submission.wait(context)).toMatchObject({
        status: "unanswered",
    });
    expect(requests).toBe(0);
    expect(await openWaits(home)).toEqual([]);
});

it("refuses questions without a supervising chief and offers object only to an instructed agent", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, "home", "scout");
    const id = await home.harness.commit(
        (tx) => startConversation(home, tx, { ...scout, chief: null, chat: 0 }),
        context,
    );
    const conversation = await home.harness.conversation(id, context);
    if (!conversation) throw new Error("No conversation");
    faux.setResponses([
        (prompt, _options, _state, model) => {
            expect(request(prompt.messages, model).tools).not.toContain(
                "object",
            );
            return call("ask_chief", { question: "Can I?" });
        },
        fauxAssistantMessage("Done."),
    ]);
    const submission = await conversation.submit(
        {
            type: "input",
            requestId: "unoverseen",
            content: "Read it.",
            entry: {
                kind: QuestionEntry.kind,
                data: { from: MAYOR, task: "unoverseen", at: home.now() },
            },
        },
        context,
    );
    expect(await submission.wait(context)).toMatchObject({ status: "done" });
    expect(await openWaits(home)).toEqual([]);
    expect(JSON.stringify(await transcript(conversation))).toContain(
        "No chief oversees this turn",
    );
});

it("keeps a late mayor's objection for dismissal after the work already ended", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, "home", "scout");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).asking.mayorReview = "always";
    }, context);
    let release = () => {};
    const released = new Promise<void>((resolve) => {
        release = resolve;
    });
    fauxAgents(faux, {
        "home:1": [fauxAssistantMessage("Done.")],
        errands: [
            async () => {
                await released;
                return fauxAssistantMessage("OBJECT: Keep the only copy.");
            },
        ],
    });
    const submission = await submitLine(
        home,
        { agent: scout, chief: CHIEF },
        { id: randomUUID(), text: "Delete it." },
        context,
    );
    expect(await submission.wait(context)).toMatchObject({ status: "done" });
    release();
    const wait = await waiting(home);
    expect(wait.answers).toEqual(["dismiss"]);
    expect(wait.about).toContain("(The work had already ended.)");
    await rule(home, wait, "dismiss");
});

it("changes an unanswered mayor objection to dismissal when its work ends", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, "home", "scout");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).asking.mayorReview = "always";
    }, context);
    fauxAgents(faux, {
        "home:1": [
            async () => {
                await waiting(home);
                return fauxAssistantMessage("Done.");
            },
        ],
        errands: [fauxAssistantMessage("OBJECT: Keep the only copy.")],
    });
    const submission = await submitLine(
        home,
        { agent: scout, chief: CHIEF },
        { id: randomUUID(), text: "Delete it." },
        context,
    );
    const wait = await waiting(home);
    expect(wait.answers).toEqual(["go", "withdraw"]);
    expect(await submission.wait(context)).toMatchObject({ status: "done" });
    await eventually(async () =>
        expect((await openWaits(home))[0]).toMatchObject({
            answers: ["dismiss"],
            about: expect.stringContaining(
                "(The work ended before you answered.)",
            ),
        }),
    );
});

it("stops a mayor-withdrawn instruction without waiting for another tool call", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, "home", "scout");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).asking.mayorReview = "always";
    }, context);
    let release = () => {};
    const released = new Promise<void>((resolve) => {
        release = resolve;
    });
    fauxAgents(faux, {
        "home:1": [
            async () => {
                await released;
                return fauxAssistantMessage("Done.");
            },
        ],
        errands: [fauxAssistantMessage("OBJECT: Keep the only copy.")],
    });
    const submission = await submitLine(
        home,
        { agent: scout, chief: CHIEF },
        { id: randomUUID(), text: "Delete it." },
        context,
    );
    await rule(home, await waiting(home), "withdraw", "Keep it.");
    try {
        await eventually(async () =>
            expect(
                JSON.stringify(await transcript(await chatWith(home, scout))),
            ).toContain("withdrew the instruction: Keep it."),
        );
    } finally {
        release();
    }
    expect(await submission.wait(context)).toMatchObject({
        status: "unanswered",
    });
});

it("hears both parties once and a go ahead clears both objections", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, "home", "scout");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).asking.mayorReview = "always";
    }, context);
    fauxAgents(faux, {
        "home:1": [
            async () => {
                await waiting(home);
                return call("object", { reason: "The only copy." });
            },
            call("object", { reason: "Still no." }),
            fauxAssistantMessage("Done."),
        ],
        errands: [fauxAssistantMessage("OBJECT: The scout needs it.")],
    });
    const submission = await submitLine(
        home,
        { agent: scout, chief: CHIEF },
        { id: randomUUID(), text: "Delete it." },
        context,
    );
    await eventually(async () => expect(await openWaits(home)).toHaveLength(2));
    const mayor = (await openWaits(home)).find((w) => w.by === "mayor");
    if (!mayor) throw new Error("No mayor objection");
    await rule(home, mayor, "go", "A backup exists.");
    expect(await submission.wait(context)).toMatchObject({ status: "done" });
    expect(await openWaits(home)).toEqual([]);
    expect(
        JSON.stringify(await transcript(await chatWith(home, scout))),
    ).toContain("may not object");
});

it.each([
    ["http://localhost:11434/v1", 0],
    ["http://127.0.0.1:8080", 0],
    ["http://[::1]:11434/v1", 0],
    ["http://192.168.1.20:11434/v1", 1],
])(
    "reviews only a mayor served from another machine in remote mode (%s)",
    async (baseUrl, expected) => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), { models });
        const scout = await addAgent(home, "home", "scout");
        await home.harness.commit(async (tx) => {
            (await tx.doc(SettingsDoc)).endpoints = [{ name: "faux", baseUrl }];
        }, context);
        let reviews = 0;
        fauxAgents(faux, {
            "home:1": [fauxAssistantMessage("Done.")],
            errands: [
                () => {
                    reviews++;
                    return fauxAssistantMessage("pass");
                },
            ],
        });
        const submission = await submitLine(
            home,
            { agent: scout, chief: CHIEF },
            { id: randomUUID(), text: "Read it." },
            context,
        );
        expect(await submission.wait(context)).toMatchObject({
            status: "done",
        });
        await home.harness.waitForIdle(context);
        expect(reviews).toBe(expected);
    },
);

it("restores a mayor's held instruction without buying another review", async () => {
    const { models, faux } = fauxModels();
    const dir = scratch();
    const before = await testHome(dir, {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    const scout = await addAgent(before, "home", "scout");
    await before.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).asking.mayorReview = "always";
    }, context);
    let reviews = 0;
    fauxAgents(faux, {
        "home:1": [
            async () => {
                await waiting(before);
                return call("write", { path: "done.txt", content: "done" });
            },
            fauxAssistantMessage("Done."),
        ],
        errands: [
            () => {
                reviews++;
                return fauxAssistantMessage("OBJECT: The only copy.");
            },
        ],
    });
    const submission = await submitLine(
        before,
        { agent: scout, chief: CHIEF },
        { id: randomUUID(), text: "Delete it." },
        context,
    );
    const wait = await waiting(before);
    await eventually(async () =>
        expect(
            JSON.stringify(await transcript(await chatWith(before, scout))),
        ).toContain('"path":"done.txt"'),
    );
    await before.close(context);
    const after = await testHome(dir, {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    expect(await waiting(after)).toMatchObject({ id: wait.id, asked: 1 });
    await rule(after, wait, "go");
    expect(
        await (await after.harness.submission(submission.id, context))?.wait(
            context,
        ),
    ).toMatchObject({ status: "done" });
    expect(reviews).toBe(1);
    expect(existsSync(join(houseOf(after.layout, scout), "done.txt"))).toBe(
        true,
    );
});

it("keeps a mayor's go ahead when the chief answers on a later day", async () => {
    const { models, faux } = fauxModels();
    let now = new Date(2026, 9, 14, 23, 59).getTime();
    const home = await testHome(scratch(), { models, now: () => now });
    const scout = await addAgent(home, "home", "scout");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).asking.mayorReview = "always";
    }, context);
    let second = false;
    fauxAgents(faux, {
        "home:1": [
            async () => {
                await waiting(home);
                second = true;
                return call("read", { path: "NOTES.md" });
            },
            call("object", { reason: "Still no." }),
            fauxAssistantMessage("Done."),
        ],
        errands: [fauxAssistantMessage("OBJECT: The only copy.")],
    });
    const submission = await submitLine(
        home,
        { agent: scout, chief: CHIEF },
        { id: randomUUID(), text: "Delete it." },
        context,
    );
    const wait = await waiting(home);
    await eventually(() => expect(second).toBe(true));
    now = new Date(2026, 9, 17, 8).getTime();
    await rule(home, wait, "go");
    expect(await submission.wait(context)).toMatchObject({ status: "done" });
    expect(await openWaits(home)).toEqual([]);
    expect(
        JSON.stringify(await transcript(await chatWith(home, scout))),
    ).toContain("may not object");
});

it("names another hamlet's mayor in a question to the chief", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const mill = await addHamlet(home, "mill", "miller");
    faux.setResponses([
        call("ask_chief", { question: "Which mill?" }),
        fauxAssistantMessage("Done."),
    ]);
    const submission = await submitLine(
        home,
        { agent: mill, chief: CHIEF },
        { id: randomUUID(), text: "Read it." },
        context,
    );
    const wait = await waiting(home);
    expect(wait.by).toBe("mayor@mill");
    await rule(home, wait, "reply", "The water mill.");
    expect(await submission.wait(context)).toMatchObject({ status: "done" });
});

it("resumes an interrupted review only after the home's model providers are ready", async () => {
    const { models, faux } = fauxModels();
    const dir = scratch();
    const before = await testHome(dir, {
        models,
        openStorage: crashing((writes) =>
            writes.some(
                (w) => w.type === "entry" && w.value.kind === "hamlet.errand",
            ),
        ),
    });
    const scout = await addAgent(before, "home", "scout");
    await before.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).asking.mayorReview = "always";
    }, context);
    fauxAgents(faux, {
        "home:1": [
            fauxAssistantMessage("Done."),
            fauxAssistantMessage("Done."),
        ],
        errands: [fauxAssistantMessage("OBJECT: The only copy.")],
    });
    await submitLine(
        before,
        { agent: scout, chief: CHIEF },
        { id: randomUUID(), text: "Delete it." },
        context,
    );
    expect(await before.harness.closed).toMatchObject({ reason: "failed" });
    const fresh = createModels();
    const provider = models.getProvider("faux");
    if (!provider) throw new Error("No faux provider");
    const after = await testHome(dir, {
        models: fresh,
        parts: [
            () => ({
                start: async () => {
                    await new Promise((resolve) => setTimeout(resolve, 200));
                    fresh.setProvider(provider);
                },
            }),
        ],
    });
    const wait = await waiting(after);
    expect(wait.kind).toBe("objection");
    expect(wait.about).toContain("The only copy.");
    await rule(after, wait, wait.answers.includes("go") ? "go" : "dismiss");
});

it.each(["question", "approval"])(
    "lapses a %s wait when the mayor's objection withdraws its turn",
    async (kind) => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), { models });
        const scout = await addAgent(home, "home", "scout");
        await home.harness.commit(async (tx) => {
            (await tx.doc(LimitsDoc, "home", "home")).asking.mayorReview =
                "always";
        }, context);
        fauxAgents(faux, {
            "home:1": [
                kind === "question"
                    ? call("ask_chief", { question: "Which list?" })
                    : fauxAssistantMessage(
                          fauxToolCall("bash", {
                              command: "printf wrong > withdrawn.txt",
                              network: true,
                          }),
                          { stopReason: "toolUse" },
                      ),
                fauxAssistantMessage("New task heard."),
            ],
            errands: [
                async () => {
                    await eventually(async () =>
                        expect(
                            (await openWaits(home)).some(
                                (w) => w.kind === kind,
                            ),
                        ).toBe(true),
                    );
                    return fauxAssistantMessage("OBJECT: Keep it.");
                },
                fauxAssistantMessage("pass"),
            ],
        });
        const submission = await submitLine(
            home,
            { agent: scout, chief: CHIEF },
            { id: randomUUID(), text: "Delete it." },
            context,
        );
        await eventually(async () =>
            expect(await openWaits(home)).toHaveLength(2),
        );
        const objection = (await openWaits(home)).find(
            (w) => w.kind === "objection",
        );
        if (!objection) throw new Error("No objection");
        await rule(home, objection, "withdraw", "Keep it.");
        expect(await submission.wait(context)).toMatchObject({
            status: "unanswered",
        });
        expect(await openWaits(home)).toEqual([]);
        expect(
            existsSync(join(houseOf(home.layout, scout), "withdrawn.txt")),
        ).toBe(false);
        const next =
            Promise.withResolvers<
                import("@earendil-works/pi-durable").Submission
            >();
        const sent = await typed(
            home,
            {
                type: "say",
                id: randomUUID(),
                to: { hamlet: "home", agent: "scout" },
                text: "Now tidy the shelf instead.",
            },
            context,
            (submission) => next.resolve(submission),
        );
        expect(sent).not.toMatchObject({
            kind: "notice",
            text: expect.stringContaining("Answered /reply"),
        });
        expect(await (await next.promise).wait(context)).toMatchObject({
            status: "done",
        });
        expect(
            JSON.stringify(await transcript(await chatWith(home, scout))),
        ).toContain("New task heard.");
    },
);

it("holds a letter receiver's work while the instructed agent's own objection is open", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    const scout = await addAgent(home, "home", "scout");
    const clerk = await addAgent(home, "home", "clerk");
    const { RecordDoc } = await import("../src/record.ts");
    await home.harness.commit(async (tx) => {
        (await tx.doc(RecordDoc, "home", "home")).roads.push({
            a: scout.number,
            b: clerk.number,
            since: home.now(),
            by: "chief:chief",
            messages: 0,
            lastFrom: null,
            lastUsed: 0,
        });
    }, context);
    const attempted = Promise.withResolvers<void>();
    fauxAgents(faux, {
        "home:1": [
            call("tell_agent", { name: "clerk", message: "Delete your copy" }),
            call("object", { reason: "Keep the list" }),
            fauxAssistantMessage("Done"),
        ],
        "home:2": [
            async () => {
                await waiting(home);
                attempted.resolve();
                return call("write", { path: "clerk.txt", content: "deleted" });
            },
            fauxAssistantMessage("Done"),
        ],
    });
    const sub = await submitLine(
        home,
        { agent: scout, chief: CHIEF },
        { id: randomUUID(), text: "Delete it everywhere" },
        context,
    );
    const objection = await waiting(home);
    await attempted.promise;
    const { PeersDoc } = await import("../src/letters.ts");
    const peer = (await home.harness.snapshot(PeersDoc, "home:2", context))
        ?.letters["home:1"];
    if (peer === undefined) throw new Error("No letter");
    await eventually(async () =>
        expect(JSON.stringify(await transcript(peer, home.harness))).toContain(
            '"path":"clerk.txt"',
        ),
    );
    await new Promise((resolve) => setTimeout(resolve, 200));
    expect(existsSync(join(houseOf(home.layout, clerk), "clerk.txt"))).toBe(
        false,
    );
    await rule(home, objection, "withdraw", "Keep it");
    expect(await sub.wait(context)).toMatchObject({ status: "unanswered" });
    await home.harness.waitForIdle(context);
    expect(existsSync(join(houseOf(home.layout, clerk), "clerk.txt"))).toBe(
        false,
    );
});

it.each([false, true])(
    "keeps go and withdraw while a letter receiver still works after the instructed turn ends (late review: %s)",
    async (lateReview) => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        const scout = await addAgent(home, "home", "scout");
        const clerk = await addAgent(home, "home", "clerk");
        const { RecordDoc } = await import("../src/record.ts");
        await home.harness.commit(async (tx) => {
            (await tx.doc(LimitsDoc, "home", "home")).asking.mayorReview =
                "always";
            (await tx.doc(RecordDoc, "home", "home")).roads.push({
                a: scout.number,
                b: clerk.number,
                since: home.now(),
                by: "chief:chief",
                messages: 0,
                lastFrom: null,
                lastUsed: 0,
            });
        }, context);
        const received = Promise.withResolvers<void>();
        const chiefEnded = Promise.withResolvers<void>();
        const rootEnded = Promise.withResolvers<void>();
        fauxAgents(faux, {
            "home:1": [
                call("tell_agent", {
                    name: "clerk",
                    message: "Delete your copy",
                }),
                async () => {
                    if (!lateReview) await waiting(home);
                    return fauxAssistantMessage("Delegated");
                },
                fauxAssistantMessage("Received reply"),
            ],
            "home:2": [
                async () => {
                    received.resolve();
                    await rootEnded.promise;
                    return call("write", {
                        path: "clerk.txt",
                        content: "deleted",
                    });
                },
                fauxAssistantMessage("Done"),
            ],
            errands: [
                async () => {
                    await received.promise;
                    if (lateReview) await chiefEnded.promise;
                    return fauxAssistantMessage("OBJECT: Keep it.");
                },
            ],
        });
        const sub = await submitLine(
            home,
            { agent: scout, chief: CHIEF },
            { id: randomUUID(), text: "Delete it everywhere" },
            context,
        );
        try {
            expect(await sub.wait(context)).toMatchObject({ status: "done" });
            chiefEnded.resolve();
            await new Promise((resolve) => setTimeout(resolve, 200));
            const objection = await waiting(home);
            expect(objection.answers).toEqual(["go", "withdraw"]);
            expect(objection.about).not.toContain("work ended");
            rootEnded.resolve();
            await rule(home, objection, "withdraw", "Keep it");
            await home.harness.waitForIdle(context);
            expect(
                existsSync(join(houseOf(home.layout, clerk), "clerk.txt")),
            ).toBe(false);
            expect(await openWaits(home)).toEqual([]);
        } finally {
            rootEnded.resolve();
            chiefEnded.resolve();
        }
    },
);

it("lets a chief-overseen retry ask the chief instead of guessing", async () => {
    const { retry } = await import("../src/chats.ts");
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    faux.setResponses([
        fauxAssistantMessage("", {
            stopReason: "error",
            errorMessage: "Interrupted",
        }),
        call("ask_chief", { question: "Paper or ebook?" }),
        fauxAssistantMessage("Ordered paper."),
    ]);
    expect(await (await say(home, "Order Dune")).wait(context)).toMatchObject({
        status: "unanswered",
    });
    const sub = await retry(home, { agent: MAYOR, chief: CHIEF }, context);
    await eventually(async () =>
        expect((await openWaits(home)).some((w) => w.kind === "question")).toBe(
            true,
        ),
    );
    const question = (await openWaits(home)).find((w) => w.kind === "question");
    if (!question) throw new Error("No question");
    await rule(home, question, "reply", "Paper.");
    expect(await sub.wait(context)).toMatchObject({ status: "done" });
    expect(JSON.stringify(await transcript(await mayorChat(home)))).toContain(
        "The chief answers: Paper.",
    );
});
