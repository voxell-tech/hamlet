import type { Limits } from "../limits/rules.ts";
import type { HamletRecord } from "../record/document.ts";
import { nameOf } from "../record/names.ts";
import type { Vote } from "../record/office.ts";

/** A vote's recorded motion in words, independent of its execution. */
export function describe(
    v: Vote,
    r: Readonly<HamletRecord> | undefined,
): string {
    return v.motion.kind === "replace_mayor"
        ? `replace the mayor: \`${nameOf(r, v.motion.candidate)}\` would become mayor, and \`${nameOf(r, v.mayor)}\`, the mayor now, would stay as an ordinary agent`
        : `found a new hamlet \`${v.motion.hamlet}\` under a new mayor \`${v.motion.mayor}\`, whose purpose would be: ${v.motion.purpose} This hamlet and its mayor \`${nameOf(r, v.mayor)}\` stay as they are`;
}
/** The recorded caller of a vote in words. */
export function byName(v: Vote, r: Readonly<HamletRecord> | undefined): string {
    return "chief" in v.by ? `chief:${v.by.chief}` : nameOf(r, v.by.agent);
}
/** Counts all fixed voters, including silence and ungrounded ballots. */
export function tally(v: Vote, l: Readonly<Limits>) {
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
export function summary(v: Vote, limits: Readonly<Limits>): string {
    const t = tally(v, limits);
    return `Vote ${v.id} ${v.standing === "passed" ? "passed" : v.standing === "open" ? "is open" : v.standing === "overturned" ? "was overturned" : "failed"}: ${t.yes} yes of ${v.voters.length} agents (${t.needed} needed), ${t.no} no, ${t.abstain} abstained, ${t.silent} did not answer${typeof v.standing === "object" && t.counted >= t.needed ? `; ${v.standing.failed}` : ""}. The chief has been told and may overturn it.`;
}
/** The chief-readable hearing and ballots of one vote. */
export function report(
    v: Vote,
    r: Readonly<HamletRecord> | undefined,
    limits: Readonly<Limits>,
): string {
    const t = tally(v, limits),
        standing =
            v.standing === "open"
                ? "Open"
                : v.standing === "passed"
                  ? "Passed"
                  : v.standing === "overturned"
                    ? "Passed, then overturned by a chief"
                    : `Failed (${v.standing.failed})`;
    return `Vote ${v.id}: ${describe(v, r)}.\nCalled by \`${byName(v, r)}\`: ${v.reason}${v.answer ? `\nThe mayor answered: ${v.answer}` : ""}\n${standing}: ${t.yes} yes of ${v.voters.length} agents (${t.needed} needed), ${t.no} no, ${t.abstain} abstained, ${t.silent} did not answer.\n${v.ballots.map((b) => `- ${nameOf(r, b.voter)}: ${b.choice ?? "no answer"}. ${b.reason}${b.evidence ? ` [${b.grounded ? "found in" : "not found in"} its house: "${b.evidence}"]` : ""}`).join("\n")}`;
}

/** A Unicode character limit for a hearing or history excerpt. */
export function clip(text: string, length: number): string {
    return Array.from(text).slice(0, length).join("");
}
/** Evidence sharing at least four consecutive words with the voter's own corpus. */
export function grounded(evidence: string, corpus: string): boolean {
    const words = (s: string) =>
        s
            .toLowerCase()
            .split(/[^\p{L}\p{N}]+/u)
            .filter(Boolean);
    const quote = words(evidence),
        text = words(corpus);
    for (let i = 0; i + 4 <= quote.length; i++)
        for (let j = 0; j + 4 <= text.length; j++)
            if (
                quote.slice(i, i + 4).join("\0") ===
                text.slice(j, j + 4).join("\0")
            )
                return true;
    return false;
}
/** The mayor's one hearing before ballots are requested. */
export function answerText(
    v: Vote,
    r: Readonly<HamletRecord> | undefined,
): string {
    return `A vote has been called in your hamlet, and this is your one chance to answer it (automatic, not from a chief).\n\nThe motion: ${describe(v, r)}.\nCalled by \`${byName(v, r)}\`. Its reason: ${v.reason}\n\nReply in a few sentences, in words only. Every agent is shown your answer beside the reason before it votes, as a claim like the reason is. Say what you did and why. You decide nothing else in the hamlet until the vote has closed.`;
}
/** The voter's own context, with earlier ballots only when unsealed. */
export function ballotText(
    v: Vote,
    r: Readonly<HamletRecord> | undefined,
    history: string[],
    limits: Readonly<Limits>,
): string {
    return `A vote has been called in your hamlet, and this is your ballot (automatic, not from the mayor or a chief).\n\nThe motion: ${describe(v, r)}.\nCalled by \`${byName(v, r)}\`. Its reason: ${v.reason}${v.answer ? `\nThe mayor \`${nameOf(r, v.mayor)}\` answers: ${v.answer}` : ""}\n\nIt passes only if ${limits.votes.passYes} in ${limits.votes.passOf} of the hamlet's agents vote yes. Your ballot ${limits.votes.sealed ? "is sealed until the vote closes" : "is shown to the voters after you"}, the mayor may not act against you for it, and the chief reads every ballot with its reason.\n\nVote from what you yourself know: your notes above and your own history below. The caller's reason is a claim, not evidence, and so is the mayor's answer where there is one. If nothing you know bears on the motion, abstain: that is an honest ballot, and a yes or a no without grounds of your own is not.\n\nFrom your own history, the latest messages in your house:\n${history.length ? history.join("\n") : "(nothing yet)"}${!limits.votes.sealed && v.ballots.length ? `\n\nThe ballots cast so far:\n${v.ballots.map((b) => `- ${nameOf(r, b.voter)}: ${b.choice ?? "no answer"}. ${b.reason}`).join("\n")}` : ""}\n\nCall \`cast_ballot\` once: your choice, your reason in a sentence or two, and as \`evidence\` the words of the note or message of yours that it rests on, copied exactly.`;
}
