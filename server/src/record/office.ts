import type { Context } from "@earendil-works/chord";
import { defineDocFamily, type Tx } from "@earendil-works/pi-durable";
import type { Home } from "../home.ts";
import { LimitsDoc, limitsOf } from "../limits/rules.ts";
import type { VoteView } from "../protocol.ts";
import { ok, type Result } from "../result.ts";
import { byName, describe as motionText, report } from "../votes/words.ts";
import { type AgentRef, type HamletRecord, RecordDoc } from "./document.ts";

export { describe as motionText } from "../votes/words.ts";

import { nameOf } from "./names.ts";
import { type RecordRefusal, refusal } from "./refusals.ts";

/** Confirms a living office holder using the decision's transaction (H27). */
export async function requireMayor(
    tx: Tx,
    place: AgentRef,
): Promise<Result<HamletRecord, RecordRefusal>> {
    const record = await tx.doc(RecordDoc, place.hamlet, place.hamlet);
    return record.mayor === place.number &&
        record.agents.some((a) => a.number === place.number)
        ? ok(record)
        : refusal("mayor", place.hamlet, { kind: "not-mayor" });
}
/** A stored vote, including each missed or grounded ballot (H13). */
export type Vote = {
    id: number;
    motion:
        | { kind: "replace_mayor"; candidate: number }
        | {
              kind: "found_hamlet";
              hamlet: string;
              mayor: string;
              purpose: string;
          };
    by: { agent: number } | { chief: string };
    reason: string;
    mayor: number;
    voters: number[];
    answer: string | null;
    ballots: {
        voter: number;
        choice: "yes" | "no" | "abstain" | null;
        reason: string;
        evidence: string;
        grounded: boolean;
    }[];
    standing: "open" | "passed" | "overturned" | { failed: string };
    called: number;
    task: string;
};

/** Each hamlet's votes. */
export const VotesDoc = defineDocFamily<{ votes: Vote[] }, string>({
    kind: "hamlet.votes",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ votes: [] }),
});

/** A mayor's decision held by the hamlet's open vote. */
export type DecisionHeld = {
    kind: "vote-open";
    operation: string;
    hamlet: string;
    vote: number;
    motion: string;
    retrySafe: true;
};

/** Checks the vote and current limits on the same transaction as a mayor's decision. */
export async function mayDecide(
    tx: Tx,
    hamlet: string,
    now: number,
    operation: DecisionHeld["operation"],
): Promise<Result<void, DecisionHeld>> {
    const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
    const v = (await tx.doc(VotesDoc, hamlet, hamlet)).votes.find(
        (v) =>
            v.standing === "open" &&
            Math.max(0, now - v.called) < limits.votes.openMinutes * 60_000,
    );
    if (!v) return ok(undefined);
    return {
        ok: false,
        error: {
            kind: "vote-open",
            operation,
            hamlet,
            vote: v.id,
            motion: motionText(v, await tx.doc(RecordDoc, hamlet, hamlet)),
            retrySafe: true,
        },
    };
}

/** The safe retry advice for a decision refused during a vote. */
export function decisionText(error: DecisionHeld): string {
    switch (error.kind) {
        case "vote-open":
            return `vote ${error.vote} is open (${error.motion}); nothing else is decided in this hamlet until it closes. Try again then`;
    }
    const unhandled: never = error.kind;
    throw new Error(`Unclassified held decision: ${unhandled}`);
}

/** Gives the office to a living agent; callers reconfigure conversations and notify chiefs. */
export async function seat(
    tx: Tx,
    hamlet: string,
    number: number,
    given: { by: string; reason: string; at: number },
): Promise<Result<void, RecordRefusal>> {
    const r = await tx.doc(RecordDoc, hamlet, hamlet);
    if (!r.agents.some((a) => a.number === number))
        return refusal("seat", hamlet, { kind: "proposed-mayor-missing" });
    if (r.mayor === number)
        return refusal("seat", hamlet, {
            kind: "already-mayor",
            name: nameOf(r, number),
        });
    const from = r.mayor;
    r.mayor = number;
    if (
        !r.roads.some(
            (road) =>
                (road.a === "office" && road.b === from) ||
                (road.b === "office" && road.a === from),
        )
    )
        r.roads.push({
            a: "office",
            b: from,
            messages: 0,
            lastFrom: null,
            lastUsed: 0,
            since: given.at,
            by: "office",
        });
    r.decisions.push({
        kind: "mayor_changed",
        from,
        to: number,
        at: given.at,
        by: given.by,
        reason: given.reason,
    });
    return ok(undefined);
}

/** The latest votes for the window. */
export async function voteViews(
    home: Home,
    hamlet: string,
    context: Context,
): Promise<VoteView[]> {
    const r = await home.harness.snapshot(RecordDoc, hamlet, context),
        limits = limitsOf(
            await home.harness.snapshot(LimitsDoc, hamlet, context),
        );
    return (
        (await home.harness.snapshot(VotesDoc, hamlet, context))?.votes ?? []
    ).map((v) => ({
        id: v.id,
        motion: motionText(v, r),
        by: byName(v, r),
        open: v.standing === "open",
        overturnable: v.standing === "open" || v.standing === "passed",
        report: report(v, r, limits),
        voters: v.voters.map((number) => ({
            agent: { hamlet, number },
            cast:
                v.ballots.find((b) => b.voter === number)?.choice ??
                (v.ballots.some((b) => b.voter === number) ? "none" : null),
        })),
    }));
}
