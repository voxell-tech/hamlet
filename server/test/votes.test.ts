import { randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
    type FauxResponseFactory,
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { ChatDoc, submitLine } from "../src/chats.ts";
import { houseOf, localDay } from "../src/home.ts";
import { LimitsDoc } from "../src/limits.ts";
import { RealmDoc, roadsOf } from "../src/realm.ts";
import { HOME, RecordDoc } from "../src/record.ts";
import { ledgerOf } from "../src/spend.ts";
import {
    vote as callVote,
    overturn,
    mayor as seatMayor,
    VotesDoc,
    voteOpen,
    votes,
} from "../src/votes.ts";
import { mayor, request, served } from "./connection-support.ts";
import {
    addAgent,
    context,
    fauxAgents,
    fauxModels,
    request as modelRequest,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

const deed = { hamlet: HOME, chief: "chief", rest: "scout A new direction" };

async function settledVote(
    ...args: Parameters<typeof callVote>
): Promise<string> {
    const [home, given, context] = args;
    await callVote(...args);
    const id = (
        await home.harness.snapshot(VotesDoc, given.hamlet, context)
    )?.votes.at(-1)?.id;
    await vi.waitFor(
        async () => {
            const v = (
                await home.harness.snapshot(VotesDoc, given.hamlet, context)
            )?.votes.find((v) => v.id === id);
            expect(v?.standing).not.toBe("open");
        },
        { timeout: 10_000 },
    );
    return votes(home, given, context);
}

const blockUntilAbort: FauxResponseFactory = (_p, options) =>
    new Promise((_resolve, reject) => {
        const signal = options?.signal;
        const stop = () => reject(new Error("stopped"));
        if (!signal || signal.aborted) stop();
        else signal.addEventListener("abort", stop, { once: true });
    });

async function hamlet(
    names = ["scout", "smith", "poet"],
    now: () => number = Date.now,
) {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models, now });
    for (const name of names) {
        const agent = await addAgent(home, HOME, name, "Works.");
        await writeFile(
            join(houseOf(home.layout, agent), "NOTES.md"),
            "# Notes\nThe mayor ignored my road request.\n",
        );
    }
    return { home, faux, models };
}

function ballots(yes: number[], silent: number[] = []): FauxResponseFactory {
    return (prompt) => {
        const text = JSON.stringify(prompt);
        if (text.includes("one chance to answer"))
            return fauxAssistantMessage("I laid the roads.");
        const number = Number(/\/houses\/home\/(\d+)/.exec(text)?.[1]);
        if (silent.includes(number))
            return fauxAssistantMessage("I have nothing to add.");
        return fauxAssistantMessage(
            fauxToolCall("cast_ballot", {
                choice: yes.includes(number) ? "yes" : "abstain",
                reason: yes.includes(number) ? "A new direction" : "",
                evidence: "the mayor ignored my road request",
            }),
            { stopReason: "toolUse" },
        );
    };
}

it("needs two thirds of every voter, so silence is not a yes", async () => {
    const { home, faux } = await hamlet(["scout", "smith", "poet", "scribe"]);
    const ballot = ballots([1, 2], [4]);
    fauxAgents(faux, { errands: [ballot, ballot, ballot, ballot, ballot] });
    expect(await settledVote(home, deed, context)).toContain(
        "2 yes of 4 agents (3 needed), 0 no, 1 abstained, 1 did not answer.",
    );
    expect((await home.harness.snapshot(RecordDoc, HOME, context))?.mayor).toBe(
        0,
    );
    expect(await votes(home, deed, context)).toContain("3 needed");
}, 30_000);

it("passes, moves the office without removing its old holder, and a chief overturns it once", async () => {
    const { home, faux } = await hamlet();
    const ballot = ballots([1, 2]);
    fauxAgents(faux, { errands: [ballot, ballot, ballot, ballot] });
    expect(await settledVote(home, deed, context)).toContain(
        "Passed: 2 yes of 3 agents (2 needed)",
    );
    const r = await home.harness.snapshot(RecordDoc, HOME, context);
    expect(r?.mayor).toBe(1);
    expect(r?.agents.map((a) => a.number)).toEqual([0, 1, 2, 3]);
    expect(r && roadsOf(r)).toEqual(
        expect.arrayContaining([
            expect.objectContaining({ a: "office", b: 0 }),
        ]),
    );
    expect(
        await overturn(home, { ...deed, rest: "1 Not yet" }, context),
    ).toContain("`founder` is the mayor again");
    expect((await home.harness.snapshot(RecordDoc, HOME, context))?.mayor).toBe(
        0,
    );
    await expect(
        overturn(home, { ...deed, rest: "1" }, context),
    ).rejects.toThrow("already overturned");
    expect(
        (await home.harness.snapshot(VotesDoc, HOME, context))?.votes[0],
    ).toMatchObject({ standing: "overturned", answer: "I laid the roads." });
}, 30_000);

it("runs a chief's vote and mayor commands over a WebSocket", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    for (const name of ["scout", "smith", "poet"])
        await addAgent(run.home, HOME, name);
    const ballot = ballots([1, 2, 3]);
    fauxAgents(faux, { errands: [ballot, ballot, ballot, ballot] });
    const command = (text: string) =>
        request(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: mayor,
            text,
        });
    expect(await command("/vote scout A new direction")).toMatchObject({
        type: "reply",
        kind: "notice",
        text: expect.stringContaining("Vote 1 is called"),
    });
    await vi.waitFor(
        async () =>
            expect(
                (await run.home.harness.snapshot(VotesDoc, HOME, context))
                    ?.votes[0]?.standing,
            ).toBe("passed"),
        { timeout: 10_000 },
    );
    expect(await command("/votes")).toMatchObject({
        text: expect.stringContaining("The mayor answered: I laid the roads."),
    });
    expect(await command("/mayor founder Back again")).toMatchObject({
        kind: "notice",
        text: expect.stringContaining("mayor"),
    });
    expect(
        (await run.home.harness.snapshot(RecordDoc, HOME, context))?.mayor,
    ).toBe(0);
    await seatMayor(
        run.home,
        { ...deed, rest: "smith New direction" },
        context,
    );
    expect(
        (await run.home.harness.snapshot(RecordDoc, HOME, context))?.mayor,
    ).toBe(2);
}, 30_000);

it("refuses bad calls before any model is asked", async () => {
    const { home, faux } = await hamlet(["scout", "smith"]);
    await expect(settledVote(home, deed, context)).rejects.toThrow(
        "at least 3 agents",
    );
    await addAgent(home, HOME, "poet");
    await expect(
        settledVote(home, { ...deed, rest: "mayor x" }, context),
    ).rejects.toThrow("mayor already");
    await expect(
        settledVote(home, { ...deed, rest: "nobody x" }, context),
    ).rejects.toThrow("candidate must be an agent");
    await home.harness.commit(async (tx) => {
        const h = (await tx.doc(RealmDoc)).hamlets.find((h) => h.id === HOME);
        if (h) h.chiefs = ["chief"];
    }, context);
    await expect(
        settledVote(home, { ...deed, chief: "outsider" }, context),
    ).rejects.toThrow("Only a chief");
    fauxAgents(faux, {
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("call_vote", {
                    kind: "replace_mayor",
                    candidate: "scout",
                    reason: " ",
                }),
                { stopReason: "toolUse" },
            ),
            (prompt) => {
                expect(JSON.stringify(prompt)).toContain("say why");
                return fauxAssistantMessage("done");
            },
        ],
    });
    await (await say(home, "call a vote")).wait(context);
    expect(
        (await home.harness.snapshot(VotesDoc, HOME, context))?.votes ?? [],
    ).toEqual([]);
    expect(faux.state.callCount).toBe(2);
});

it.each([true, false])(
    "checks evidence in each voter's house, and seals the ballots when requested: %s",
    async (sealed) => {
        const { home, faux } = await hamlet();
        await home.harness.commit(async (tx) => {
            const l = await tx.doc(LimitsDoc, HOME, HOME);
            l.votes.groundedOnly = true;
            l.votes.sealed = sealed;
        }, context);
        const script: FauxResponseFactory = (prompt, _o, _s, model) => {
            const sent = modelRequest(prompt.messages, model),
                text = JSON.stringify(prompt);
            if (text.includes("one chance to answer"))
                return fauxAssistantMessage("I helped every agent.");
            expect(sent.tools).toEqual(["cast_ballot"]);
            const number = Number(/\/houses\/home\/(\d+)/.exec(text)?.[1]);
            if (sealed) expect(text).not.toContain("The ballots cast so far");
            else if (number > 1)
                expect(text).toContain("The ballots cast so far");
            const evidence =
                number === 1
                    ? "THE MAYOR ignored, my road request"
                    : "I helped every agent";
            return fauxAssistantMessage(
                fauxToolCall("cast_ballot", {
                    choice: "yes",
                    reason: "Change",
                    evidence,
                }),
                { stopReason: "toolUse" },
            );
        };
        fauxAgents(faux, { errands: [script, script, script, script] });
        expect(await settledVote(home, deed, context)).toContain(
            "3 yes of 3 agents",
        );
        expect(
            (
                await home.harness.snapshot(VotesDoc, HOME, context)
            )?.votes[0]?.ballots
                .map((b) => [b.voter, b.grounded])
                .sort(),
        ).toEqual([
            [1, true],
            [2, false],
            [3, false],
        ]);
    },
    30_000,
);

it("keeps a vote open while its ballots wait, holds decisions, and lets a chief stop it", async () => {
    const { home, faux } = await hamlet();

    fauxAgents(faux, {
        errands: [
            fauxAssistantMessage("I laid roads."),
            blockUntilAbort,
            blockUntilAbort,
            blockUntilAbort,
        ],
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("remove_agent", {
                    name: "scout",
                    reason: "No work",
                }),
                { stopReason: "toolUse" },
            ),
            (prompt) => {
                expect(JSON.stringify(prompt)).toContain(
                    "nothing else is decided",
                );
                return fauxAssistantMessage("done");
            },
        ],
    });
    const pending = settledVote(home, deed, context);
    await vi.waitFor(
        async () =>
            expect(
                (await home.harness.snapshot(VotesDoc, HOME, context))?.votes[0]
                    ?.standing,
            ).toBe("open"),
        { timeout: 10_000 },
    );
    expect(await voteOpen(home.harness, HOME, home.now(), context)).toContain(
        "nothing else is decided",
    );
    await expect(
        settledVote(home, { ...deed, rest: "smith Another" }, context),
    ).rejects.toThrow("only one vote");
    await (await say(home, "remove scout")).wait(context);
    expect(
        (await home.harness.snapshot(RecordDoc, HOME, context))?.agents,
    ).toHaveLength(4);
    expect(await overturn(home, { ...deed, rest: "1 Stop now" }, context)).toBe(
        "Vote 1 is stopped.",
    );
    expect(await pending).toContain("Failed (stopped by a chief)");
    expect(
        await voteOpen(home.harness, HOME, home.now(), context),
    ).toBeUndefined();
}, 30_000);

it("allows at most three requests per ballot and charges every hearing and ballot to the vote's task", async () => {
    const { home, faux } = await hamlet();
    const requests = new Map<number, number>();
    const invalid: FauxResponseFactory = (prompt) => {
        const text = JSON.stringify(prompt);
        if (text.includes("one chance to answer"))
            return fauxAssistantMessage("I helped.");
        const number = Number(/\/houses\/home\/(\d+)/.exec(text)?.[1]);
        const n = (requests.get(number) ?? 0) + 1;
        requests.set(number, n);
        return n < 4
            ? fauxAssistantMessage(
                  fauxToolCall("cast_ballot", {
                      choice: "yes",
                      reason: "",
                      evidence: "",
                  }),
                  { stopReason: "toolUse" },
              )
            : fauxAssistantMessage("No ballot.");
    };
    fauxAgents(faux, { errands: Array.from({ length: 13 }, () => invalid) });
    expect(await settledVote(home, deed, context)).toContain(
        "3 did not answer",
    );
    expect([...requests.values()]).toEqual([3, 3, 3]);
    const v = (await home.harness.snapshot(VotesDoc, HOME, context))?.votes[0];
    const lines = await ledgerOf(home, HOME, localDay(home.now()), context);
    expect(lines).toHaveLength(10);
    expect(lines.every((l) => l.what !== "reset" && l.task === v?.task)).toBe(
        true,
    );
}, 30_000);

it("ends a ballot whose model never answers within ballotMinutes", async () => {
    const { home, faux } = await hamlet();
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, HOME, HOME)).votes.ballotMinutes = 0.005;
    }, context);

    fauxAgents(faux, {
        errands: [
            fauxAssistantMessage("I helped."),
            blockUntilAbort,
            blockUntilAbort,
            blockUntilAbort,
        ],
    });
    expect(await settledVote(home, deed, context)).toContain(
        "3 did not answer",
    );
}, 10_000);

it("an agent calls a durable vote and gets the count; the mayor's own call needs no hearing", async () => {
    const { home, faux } = await hamlet();
    const ballot = ballots([1, 2, 3]);
    fauxAgents(faux, {
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("call_vote", {
                    kind: "replace_mayor",
                    candidate: "scout",
                    reason: "New direction",
                }),
                { stopReason: "toolUse" },
            ),
            (prompt, _o, _s, model) => {
                expect(JSON.stringify(prompt)).toContain("Vote 1 passed");
                const tools = modelRequest(prompt.messages, model).tools;
                expect(tools).toContain("call_vote");
                expect(tools).not.toContain("remove_agent");
                expect(tools).not.toContain("merge_agents");
                return fauxAssistantMessage("done");
            },
        ],
        errands: [ballot, ballot, ballot],
    });
    expect((await (await say(home, "call a vote")).wait(context)).status).toBe(
        "done",
    );
    expect(
        (await home.harness.snapshot(VotesDoc, HOME, context))?.votes[0]
            ?.answer,
    ).toBeNull();
    expect(faux.state.callCount).toBe(5);
}, 30_000);

it("resumes between unsealed ballots without asking a finished voter again", async () => {
    const { home, faux, models } = await hamlet();
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, HOME, HOME)).votes.sealed = false;
    }, context);
    let firstCalls = 0;
    const ballot = ballots([1, 2, 3]);
    const first: FauxResponseFactory = (p, o, s, m) => {
        firstCalls++;
        return ballot(p, o, s, m);
    };

    fauxAgents(faux, {
        errands: [fauxAssistantMessage("I helped."), first, blockUntilAbort],
    });
    const pending = settledVote(home, deed, context).catch(() => "closed");
    await vi.waitFor(
        async () =>
            expect(
                (await home.harness.snapshot(VotesDoc, HOME, context))?.votes[0]
                    ?.ballots,
            ).toHaveLength(1),
        { timeout: 10_000 },
    );
    const dir = home.dir;
    await home.close(context);
    await pending;
    fauxAgents(faux, { errands: [ballot, ballot] });
    const again = await testHome(dir, { models });
    await vi.waitFor(
        async () =>
            expect(
                (await again.harness.snapshot(VotesDoc, HOME, context))
                    ?.votes[0]?.standing,
            ).toBe("passed"),
        { timeout: 15_000 },
    );
    expect(firstCalls).toBe(1);
    expect(
        (
            await again.harness.snapshot(VotesDoc, HOME, context)
        )?.votes[0]?.ballots.map((b) => b.voter),
    ).toEqual([1, 2, 3]);
}, 30_000);

it("does not replace a mayor the chief changed while the vote was open", async () => {
    const { home, faux } = await hamlet();
    let release = () => {};
    const ready = new Promise<void>((resolve) => {
        release = resolve;
    });
    const ballot = ballots([1, 2, 3]);
    const block: FauxResponseFactory = async (p, o, s, m) => {
        await Promise.race([ready, blockUntilAbort(p, o, s, m)]);
        return ballot(p, o, s, m);
    };
    fauxAgents(faux, {
        errands: [fauxAssistantMessage("I helped."), block, block, block],
    });
    const pending = settledVote(home, deed, context);
    await vi.waitFor(() => expect(faux.state.callCount).toBe(4), {
        timeout: 10_000,
    });
    await seatMayor(home, { ...deed, rest: "smith Chief's choice" }, context);
    release();
    expect(await pending).toContain(
        "the mayor changed while the vote was open",
    );
    expect((await home.harness.snapshot(RecordDoc, HOME, context))?.mayor).toBe(
        2,
    );
}, 30_000);

it("grounds a ballot in that voter's own history and offers no file tools", async () => {
    const { home, faux } = await hamlet();
    const scout = { hamlet: HOME, number: 1 };
    fauxAgents(faux, { "home:1": [fauxAssistantMessage("I remember.")] });
    await (
        await submitLine(
            home,
            { agent: scout, chief: home.chief },
            { id: randomUUID(), text: "Our only road was refused." },
            context,
        )
    ).wait(context);
    let calls = 0;
    const script: FauxResponseFactory = (p, o, s, m) => {
        const text = JSON.stringify(p);
        if (text.includes("one chance to answer"))
            return fauxAssistantMessage("I helped.");
        if (text.includes("/houses/home/1")) {
            expect(text).toContain("Our only road was refused.");
            expect(modelRequest(p.messages, m).tools).toEqual(["cast_ballot"]);
            if (++calls === 1)
                return fauxAssistantMessage(
                    fauxToolCall("write", {
                        path: "forbidden",
                        content: "not a ballot",
                    }),
                    { stopReason: "toolUse" },
                );
            return fauxAssistantMessage(
                fauxToolCall("cast_ballot", {
                    choice: "yes",
                    reason: "Roads",
                    evidence: "Our only road was refused",
                }),
                { stopReason: "toolUse" },
            );
        }
        return ballots([])(p, o, s, m);
    };
    fauxAgents(faux, { errands: [script, script, script, script, script] });
    await settledVote(home, deed, context);
    expect(
        (
            await home.harness.snapshot(VotesDoc, HOME, context)
        )?.votes[0]?.ballots.find((b) => b.voter === 1),
    ).toMatchObject({ choice: "yes", grounded: true });
    expect(existsSync(join(houseOf(home.layout, scout), "forbidden"))).toBe(
        false,
    );
}, 30_000);

it("lapses an old open vote before starting another and refuses a recent failed caller's retry", async () => {
    const { home, faux } = await hamlet();
    const old = {
        id: 1,
        motion: { kind: "replace_mayor" as const, candidate: 1 },
        by: { chief: home.chief },
        reason: "Old motion",
        mayor: 0,
        voters: [1, 2, 3],
        answer: null,
        ballots: [],
        standing: "open" as const,
        called: home.now() - 3_600_000,
        task: "old-vote",
    };
    await home.harness.commit(async (tx) => {
        (await tx.doc(VotesDoc, HOME, HOME)).votes.push(old);
    }, context);
    expect(
        await voteOpen(home.harness, HOME, home.now(), context),
    ).toBeUndefined();
    const ballot = ballots([1, 2, 3]);
    fauxAgents(faux, { errands: [ballot, ballot, ballot, ballot] });
    expect(await settledVote(home, deed, context)).toContain(
        "Passed: 3 yes of 3 agents",
    );
    expect(
        (await home.harness.snapshot(VotesDoc, HOME, context))?.votes[0]
            ?.standing,
    ).toEqual({ failed: "lapsed" });
    await home.harness.commit(async (tx) => {
        (await tx.doc(VotesDoc, HOME, HOME)).votes.push({
            ...old,
            id: 3,
            called: home.now(),
            standing: { failed: "too few said yes" },
        });
    }, context);
    await expect(
        settledVote(home, { ...deed, rest: "smith Again" }, context),
    ).rejects.toThrow("failed a short while ago");
}, 30_000);

it("records only the first ballot, cuts the hearing to 600 characters, and rejects ballots outside their errand", async () => {
    const { home, faux } = await hamlet();
    const asked = new Set<number>();
    const duplicate: FauxResponseFactory = (p) => {
        const text = JSON.stringify(p);
        if (text.includes("one chance to answer"))
            return fauxAssistantMessage("x".repeat(700));
        const number = Number(/\/houses\/home\/(\d+)/.exec(text)?.[1]);
        if (asked.has(number)) return fauxAssistantMessage("done");
        asked.add(number);
        return fauxAssistantMessage(
            [
                fauxToolCall("cast_ballot", {
                    choice: "yes",
                    reason: "First",
                    evidence: "the mayor ignored my road request",
                }),
                fauxToolCall("cast_ballot", {
                    choice: "no",
                    reason: "Second",
                    evidence: "",
                }),
            ],
            { stopReason: "toolUse" },
        );
    };
    const scripts = Array.from({ length: 7 }, () => duplicate);
    fauxAgents(faux, {
        errands: scripts,
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("cast_ballot", {
                    choice: "yes",
                    reason: "Not a voter",
                    evidence: "",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("done"),
        ],
    });
    await (await say(home, "cast a ballot")).wait(context);
    expect(
        (await home.harness.snapshot(VotesDoc, HOME, context))?.votes ?? [],
    ).toEqual([]);
    await settledVote(home, deed, context);
    const v = (await home.harness.snapshot(VotesDoc, HOME, context))?.votes[0];
    expect(v?.answer).toHaveLength(600);
    expect(v?.ballots).toHaveLength(3);
    expect(v?.ballots.map((b) => b.choice)).toEqual(["yes", "yes", "yes"]);
}, 30_000);

it("keeps the chief's mayor address on the new office holder, while the new mayor's own roads sleep", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    await addAgent(run.home, HOME, "scout");
    await addAgent(run.home, HOME, "smith");
    await run.home.harness.commit(async (tx) => {
        (await tx.doc(RecordDoc, HOME, HOME)).roads.push({
            a: 1,
            b: 2,
            messages: 7,
            lastFrom: 1,
            lastUsed: 1,
            since: 1,
            by: "mayor",
        });
    }, context);
    await seatMayor(run.home, deed, context);
    const r = await run.home.harness.snapshot(RecordDoc, HOME, context);
    expect(
        r &&
            roadsOf(r).some(
                (r) => (r.a === 1 && r.b === 2) || (r.a === 2 && r.b === 1),
            ),
    ).toBe(false);
    fauxAgents(faux, {
        "home:1": [fauxAssistantMessage("I am the new mayor.")],
    });
    expect(
        await request(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: mayor,
            text: "Who holds the office?",
        }),
    ).toMatchObject({ type: "reply", kind: "notice" });
    const chats = await run.home.harness.snapshot(ChatDoc, "home:1", context);
    expect(chats?.chats[run.home.chief]?.conversations).toHaveLength(1);
    const id = chats?.chats[run.home.chief]?.conversations[0]?.conversation;
    expect(
        id && JSON.stringify(await transcript(id, run.home.harness)),
    ).toContain("I am the new mayor.");
    await seatMayor(run.home, { ...deed, rest: "founder Back" }, context);
    const after = await run.home.harness.snapshot(RecordDoc, HOME, context);
    expect(after && roadsOf(after)).toContainEqual(
        expect.objectContaining({ a: 1, b: 2, messages: 7 }),
    );
}, 30_000);

it("closes and stops lapsed work before another vote asks its voters", async () => {
    let now = 1000;
    const { home, faux } = await hamlet(["scout", "smith", "poet"], () => now);

    fauxAgents(faux, {
        errands: [
            fauxAssistantMessage("I helped."),
            blockUntilAbort,
            blockUntilAbort,
            blockUntilAbort,
        ],
    });
    const old = settledVote(home, deed, context);
    await vi.waitFor(() => expect(faux.state.callCount).toBe(4), {
        timeout: 10_000,
    });
    now += 3_600_000;
    const ballot = ballots([1, 2, 3]);
    fauxAgents(faux, { errands: [ballot, ballot, ballot, ballot] });
    expect(
        await settledVote(home, { ...deed, rest: "smith New motion" }, context),
    ).toContain("Passed: 3 yes of 3 agents");
    expect(await old).toContain("Failed (lapsed)");
}, 15_000);

it("resumes call_vote itself without calling a second vote", async () => {
    const { home, faux, models } = await hamlet();
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, HOME, HOME)).votes.sealed = false;
    }, context);
    const ballot = ballots([1, 2, 3]);

    fauxAgents(faux, {
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("call_vote", {
                    kind: "replace_mayor",
                    candidate: "scout",
                    reason: "Change",
                }),
                { stopReason: "toolUse" },
            ),
        ],
        errands: [ballot, blockUntilAbort],
    });
    const submission = await say(home, "call a vote");
    await vi.waitFor(
        async () =>
            expect(
                (await home.harness.snapshot(VotesDoc, HOME, context))?.votes[0]
                    ?.ballots,
            ).toHaveLength(1),
        { timeout: 10_000 },
    );
    const dir = home.dir;
    await home.close(context);
    fauxAgents(faux, {
        errands: [ballot, ballot],
        "home:0": [fauxAssistantMessage("The vote passed.")],
    });
    const again = await testHome(dir, { models });
    expect(
        (
            await (
                await again.harness.submission(submission.id, context)
            )?.wait(context)
        )?.status,
    ).toBe("done");
    expect(
        (await again.harness.snapshot(VotesDoc, HOME, context))?.votes,
    ).toHaveLength(1);
    expect(
        (await again.harness.snapshot(VotesDoc, HOME, context))?.votes[0]
            ?.standing,
    ).toBe("passed");
}, 30_000);

it("accepts a chief's vote without holding its connection, so that chief can stop it", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    for (const name of ["scout", "smith", "poet"])
        await addAgent(run.home, HOME, name);
    fauxAgents(faux, {
        errands: [
            blockUntilAbort,
            blockUntilAbort,
            blockUntilAbort,
            blockUntilAbort,
        ],
    });
    const command = (text: string) =>
        request(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: mayor,
            text,
        });
    const called = command("/vote scout New direction");
    const stopped = command("/overturn 1 Stop now");
    expect(await called).toMatchObject({
        text: expect.stringContaining("Vote 1 is called"),
    });
    expect(await stopped).toMatchObject({ text: "Vote 1 is stopped." });
}, 5_000);
