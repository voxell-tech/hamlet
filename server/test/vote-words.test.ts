import { expect, it } from "vitest";
import { defaultLimits } from "../src/limits.ts";
import { RecordDoc, type Vote } from "../src/record.ts";
import {
    ballotText,
    grounded,
    report,
    summary,
    tally,
} from "../src/votes/words.ts";

const vote: Vote = {
    id: 1,
    motion: { kind: "replace_mayor", candidate: 1 },
    by: { chief: "chief" },
    reason: "A new direction",
    mayor: 0,
    voters: [1, 2, 3, 4],
    answer: "I laid the roads.",
    ballots: [
        {
            voter: 1,
            choice: "yes",
            reason: "Change",
            evidence: "the mayor ignored my road request",
            grounded: true,
        },
        {
            voter: 2,
            choice: "yes",
            reason: "Change",
            evidence: "I helped every agent",
            grounded: false,
        },
        {
            voter: 3,
            choice: "abstain",
            reason: "",
            evidence: "",
            grounded: false,
        },
        {
            voter: 4,
            choice: null,
            reason: "no answer",
            evidence: "",
            grounded: false,
        },
    ],
    standing: { failed: "too few said yes" },
    called: 7,
    task: "vote-task",
};

it("needs two thirds of every voter, so silence is not a yes", () => {
    const limits = defaultLimits();
    expect(tally(vote, limits)).toEqual({
        yes: 2,
        no: 0,
        abstain: 1,
        silent: 1,
        counted: 2,
        needed: 3,
    });
    expect(report(vote, undefined, limits)).toContain(
        "2 yes of 4 agents (3 needed), 0 no, 1 abstained, 1 did not answer.",
    );
    expect(summary(vote, limits)).toBe(
        "Vote 1 failed: 2 yes of 4 agents (3 needed), 0 no, 1 abstained, 1 did not answer. The chief has been told and may overturn it.",
    );
    limits.votes.groundedOnly = true;
    expect(tally(vote, limits).counted).toBe(1);
});

it.each([true, false])(
    "checks a voter's own evidence and seals ballot words when requested: %s",
    (sealed) => {
        const limits = defaultLimits();
        limits.votes.sealed = sealed;
        const notes = "# Notes\nThe mayor ignored my road request.\n";
        const own = grounded("THE MAYOR ignored, my road request", notes);
        const borrowed = grounded("I helped every agent", notes);
        expect([
            [1, own],
            [2, borrowed],
            [3, borrowed],
        ]).toEqual([
            [1, true],
            [2, false],
            [3, false],
        ]);
        const words = ballotText(
            vote,
            undefined,
            ["- chief said to you: Our only road was refused."],
            limits,
        );
        expect(words).toContain("The mayor `0` answers: I laid the roads.");
        expect(words).toContain("Our only road was refused.");
        if (sealed) expect(words).not.toContain("The ballots cast so far");
        else expect(words).toContain("The ballots cast so far");
    },
);

it("reports the hearing, voter names and an execution failure after enough said yes", () => {
    const record = RecordDoc.definition.initial("home");
    record.agents.push({
        number: 1,
        name: "scout",
        purpose: "Maps",
        since: 0,
        model: null,
    });
    const passed: Vote = {
        ...vote,
        voters: [1, 2, 3],
        standing: {
            failed: "it passed but could not be done: the candidate left the hamlet",
        },
    };
    const limits = defaultLimits();
    expect(report(passed, record, limits)).toContain(
        "The mayor answered: I laid the roads.",
    );
    expect(report(passed, record, limits)).toContain(
        '- scout: yes. Change [found in its house: "the mayor ignored my road request"]',
    );
    expect(summary(passed, limits)).toContain(
        "; it passed but could not be done: the candidate left the hamlet.",
    );
    expect(
        report({ ...passed, standing: "overturned" }, record, limits),
    ).toContain("Passed, then overturned by a chief");
});
