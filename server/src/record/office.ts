import type { Context } from "@earendil-works/chord";
import { defineDocFamily, type Tx } from "@earendil-works/pi-durable";
import type { Home } from "../home.ts";
import { type Limits, LimitsDoc, limitsOf } from "../limits/rules.ts";
import type { VoteView } from "../protocol.ts";
import { ok, type Result } from "../result.ts";
import { type AgentRef, type HamletRecord, RecordDoc } from "./document.ts";
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

/** A vote's recorded motion in words, independent of its execution. */
export function motionText(
    v: Vote,
    r: Readonly<HamletRecord> | undefined,
): string {
    return v.motion.kind === "replace_mayor"
        ? `replace the mayor: \`${nameOf(r, v.motion.candidate)}\` would become mayor, and \`${nameOf(r, v.mayor)}\`, the mayor now, would stay as an ordinary agent`
        : `found a new hamlet \`${v.motion.hamlet}\` under a new mayor \`${v.motion.mayor}\`, whose purpose would be: ${v.motion.purpose} This hamlet and its mayor \`${nameOf(r, v.mayor)}\` stay as they are`;
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

/** The recorded caller of a vote in words. */
export function voteBy(v: Vote, r: Readonly<HamletRecord> | undefined): string {
    return "chief" in v.by ? `chief:${v.by.chief}` : nameOf(r, v.by.agent);
}
/** Counts all fixed voters, including silence and ungrounded ballots. */
export function voteTally(v: Vote, l: Readonly<Limits>) {
    const yes = v.ballots.filter((b) => b.choice === "yes").length,
        no = v.ballots.filter((b) => b.choice === "no").length,
        abstain = v.ballots.filter((b) => b.choice === "abstain").length;
    return {
        yes,
        no,
        abstain,
        silent: v.voters.length - yes - no - abstain,
        counted: v.ballots.filter(
            (b) => b.choice === "yes" && (!l.votes.groundedOnly || b.grounded),
        ).length,
        needed: Math.max(
            1,
            Math.ceil((v.voters.length * l.votes.passYes) / l.votes.passOf),
        ),
    };
}
/** A vote result notice with the current threshold. */
export function voteSummary(v: Vote, limits: Readonly<Limits>): string {
    const t = voteTally(v, limits);
    return `Vote ${v.id} ${v.standing === "passed" ? "passed" : v.standing === "open" ? "is open" : v.standing === "overturned" ? "was overturned" : "failed"}: ${t.yes} yes of ${v.voters.length} agents (${t.needed} needed), ${t.no} no, ${t.abstain} abstained, ${t.silent} did not answer${typeof v.standing === "object" && t.counted >= t.needed ? `; ${v.standing.failed}` : ""}. The chief has been told and may overturn it.`;
}
/** The chief-readable hearing and ballots of one vote. */
export function voteReport(
    v: Vote,
    r: Readonly<HamletRecord> | undefined,
    limits: Readonly<Limits>,
): string {
    const t = voteTally(v, limits),
        standing =
            v.standing === "open"
                ? "Open"
                : v.standing === "passed"
                  ? "Passed"
                  : v.standing === "overturned"
                    ? "Passed, then overturned by a chief"
                    : `Failed (${v.standing.failed})`;
    return `Vote ${v.id}: ${motionText(v, r)}.\nCalled by \`${voteBy(v, r)}\`: ${v.reason}${v.answer ? `\nThe mayor answered: ${v.answer}` : ""}\n${standing}: ${t.yes} yes of ${v.voters.length} agents (${t.needed} needed), ${t.no} no, ${t.abstain} abstained, ${t.silent} did not answer.\n${v.ballots.map((b) => `- ${nameOf(r, b.voter)}: ${b.choice ?? "no answer"}. ${b.reason}${b.evidence ? ` [${b.grounded ? "found in" : "not found in"} its house: "${b.evidence}"]` : ""}`).join("\n")}`;
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
        by: voteBy(v, r),
        open: v.standing === "open",
        overturnable: v.standing === "open" || v.standing === "passed",
        report: voteReport(v, r, limits),
        voters: v.voters.map((number) => ({
            agent: { hamlet, number },
            cast:
                v.ballots.find((b) => b.voter === number)?.choice ??
                (v.ballots.some((b) => b.voter === number) ? "none" : null),
        })),
    }));
}
