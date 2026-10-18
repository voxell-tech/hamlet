import type { Context } from "@earendil-works/chord";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { Type } from "@earendil-works/pi-ai";
import {
    type ConversationId,
    defineDoc,
    defineExtension,
    defineTool,
    GenerationTask,
    hook,
    type TaskId,
    type Tx,
} from "@earendil-works/pi-durable";
import { current, tellChiefs, WithDoc } from "./chats/conversations.ts";
import { ErrandDoc } from "./chats/errands.ts";
import { newTask } from "./chats/turn.ts";
import { untilAborted } from "./home/abort.ts";
import type { Home, Part } from "./home.ts";
import { LimitsDoc } from "./limits/rules.ts";
import { isChief, liveHamlets, RealmDoc } from "./realm/places.ts";
import { mayFound, removeHamlet } from "./realm.ts";
import {
    type HamletRecord,
    memoised,
    motionText,
    named,
    nameOf,
    type RecordDeed,
    RecordDoc,
    type RecordRefusal,
    recordRefusal,
    refusedTool,
    toolText,
    type Vote,
    VotesDoc,
    voteViews,
} from "./record.ts";
import { ok, type Result } from "./result.ts";
import {
    finish,
    PollDoc,
    RunDoc,
    runKey,
    seat,
    voteTask,
} from "./votes/task.ts";
import { byName, grounded } from "./votes/words.ts";

type Motion =
    | { kind: "replace_mayor"; candidate: string }
    | { kind: "found_hamlet"; hamlet: string; mayor: string; purpose: string };
const CallDoc = defineDoc<{ hamlet: string | null; id: number | null }>({
    kind: "hamlet.vote-call",
    version: 1,
    scope: "task",
    initial: () => ({ hamlet: null, id: null }),
});
const BallotDoc = defineDoc<{ text: string | null }>({
    kind: "hamlet.ballot-call",
    version: 1,
    scope: "task",
    initial: () => ({ text: null }),
});
const RequestDoc = defineDoc<{ counted: boolean }>({
    kind: "hamlet.poll-request",
    version: 1,
    scope: "task",
    initial: () => ({ counted: false }),
});
const runner = Symbol("vote task");
type VoteHome = Home & { [runner]?: ReturnType<typeof voteTask> };

/** The hamlet's votes and office changes. */
export const Votes: Part = (home: VoteHome) => {
    const task = voteTask(home);
    home[runner] = task;
    const cast = defineTool({
        name: "cast_ballot",
        description:
            "Cast your ballot in the vote you were asked about, with words copied from your own notes or history.",
        parameters: Type.Object({
            choice: Type.String(),
            reason: Type.String(),
            evidence: Type.String(),
        }),
        replay: "safe",
        execute: async (args, api, context) => {
            const outcome = await memoised(
                api,
                {
                    read: async (tx) => {
                        const saved = (await tx.doc(BallotDoc, api.taskId))
                            .text;
                        return saved ? ok(saved) : undefined;
                    },
                    change: async (
                        tx,
                    ): Promise<Result<string, RecordRefusal>> => {
                        const poll = await tx.doc(PollDoc, api.conversationId);
                        const errand = await tx.doc(
                            ErrandDoc,
                            api.conversationId,
                        );
                        const place = await tx.doc(WithDoc, api.conversationId);
                        if (
                            poll.kind !== "ballot" ||
                            poll.hamlet === null ||
                            errand.kind !== "ballot"
                        )
                            return recordRefusal("cast-ballot", place.hamlet, {
                                kind: "ballot-not-asked",
                            });
                        const choice = args.choice.trim().toLowerCase();
                        if (
                            choice !== "yes" &&
                            choice !== "no" &&
                            choice !== "abstain"
                        )
                            return recordRefusal("cast-ballot", place.hamlet, {
                                kind: "ballot-choice",
                            });
                        if (choice !== "abstain" && !args.reason.trim())
                            return recordRefusal("cast-ballot", place.hamlet, {
                                kind: "ballot-reason",
                            });
                        const vote = (
                            await tx.doc(VotesDoc, poll.hamlet, poll.hamlet)
                        ).votes.find((v) => v.id === poll.vote);
                        if (vote?.standing !== "open")
                            return recordRefusal("cast-ballot", place.hamlet, {
                                kind: "ballot-closed",
                                vote: poll.vote,
                            });
                        if (home.now() >= poll.until)
                            return recordRefusal("cast-ballot", place.hamlet, {
                                kind: "ballot-ended",
                            });
                        if (!vote.voters.includes(place.number))
                            return recordRefusal("cast-ballot", place.hamlet, {
                                kind: "ballot-not-voter",
                                vote: vote.id,
                            });
                        if (vote.ballots.some((b) => b.voter === place.number))
                            return recordRefusal("cast-ballot", place.hamlet, {
                                kind: "ballot-already-cast",
                                vote: vote.id,
                            });
                        if (
                            !(
                                await tx.doc(
                                    RecordDoc,
                                    place.hamlet,
                                    place.hamlet,
                                )
                            ).agents.some((a) => a.number === place.number)
                        )
                            return recordRefusal("cast-ballot", place.hamlet, {
                                kind: "ballot-departed",
                            });
                        vote.ballots.push({
                            voter: place.number,
                            choice,
                            reason: args.reason.trim(),
                            evidence: args.evidence.trim(),
                            grounded: grounded(args.evidence, poll.corpus),
                        });
                        await tellChiefs(
                            home,
                            tx,
                            poll.hamlet,
                            `Vote ${vote.id}: ${nameOf(await tx.doc(RecordDoc, place.hamlet, place.hamlet), place.number)} voted ${choice}. ${args.reason.trim()}`,
                        );
                        return ok(
                            "Your ballot is recorded. The chief reads it with its reason.",
                        );
                    },
                    write: async (tx, value) => {
                        if (value.ok)
                            (await tx.doc(BallotDoc, api.taskId)).text =
                                value.value;
                    },
                },
                context,
            );
            return outcome.ok
                ? {
                      output: [{ type: "text", text: outcome.value }],
                      control: { terminate: true },
                  }
                : refusedTool(toolText(outcome.error));
        },
    });
    return {
        extension: defineExtension({
            name: "hamlet.votes",
            tasks: [task],
            hooks: [
                hook(GenerationTask, {
                    beforeRequest: async (_r, api, context) => {
                        const poll = await api.snapshot(
                            PollDoc,
                            api.conversationId,
                            context,
                        );
                        if (poll?.kind == null) return;
                        const stopped = await home.harness.commit(
                            async (tx) => {
                                const current = await tx.doc(
                                    PollDoc,
                                    api.conversationId,
                                );
                                const request = await tx.doc(
                                    RequestDoc,
                                    api.taskId,
                                );
                                if (home.now() >= current.until) return true;
                                if (request.counted) return false;
                                if (
                                    current.requests >=
                                    (current.kind === "ballot" ? 3 : 1)
                                )
                                    return true;
                                request.counted = true;
                                current.requests++;
                                return false;
                            },
                            context,
                        );
                        if (stopped) {
                            void home.harness.abortTask(
                                api.taskId,
                                BACKGROUND_CONTEXT,
                            );
                            return untilAborted(context);
                        }
                    },
                }),
            ],
            tools: [
                cast,
                defineTool({
                    name: "call_vote",
                    description:
                        "Call a vote to replace the mayor or found a hamlet. Every agent besides the mayor votes from its own house. Returns the count; the chief may overturn it.",
                    parameters: Type.Object({
                        kind: Type.String(),
                        reason: Type.String(),
                        candidate: Type.Optional(Type.String()),
                        hamlet: Type.Optional(Type.String()),
                        mayor: Type.Optional(Type.String()),
                        purpose: Type.Optional(Type.String()),
                    }),
                    replay: "safe",
                    execute: async (args, api, context) => {
                        if (
                            home.overrides.workspace !== undefined &&
                            args.kind === "found_hamlet"
                        )
                            return refusedTool(
                                "Founding another hamlet is not supported by remote workspaces. No vote was opened.",
                            );
                        const known = await api.snapshot(
                            CallDoc,
                            api.taskId,
                            context,
                        );
                        let id = known?.id;
                        let hamlet = known?.hamlet;
                        if (id == null || hamlet == null) {
                            if (
                                args.kind !== "replace_mayor" &&
                                args.kind !== "found_hamlet"
                            )
                                return refusedTool(
                                    "`kind` is replace_mayor or found_hamlet",
                                );
                            const motion: Motion =
                                args.kind === "replace_mayor"
                                    ? {
                                          kind: "replace_mayor",
                                          candidate: args.candidate ?? "",
                                      }
                                    : {
                                          kind: "found_hamlet",
                                          hamlet: args.hamlet ?? "",
                                          mayor: args.mayor ?? "",
                                          purpose: args.purpose ?? "",
                                      };
                            const place = await api.snapshot(
                                WithDoc,
                                api.conversationId,
                                context,
                            );
                            if (!place)
                                return refusedTool(
                                    "You do not live in a hamlet.",
                                );
                            const expired = await expire(
                                home,
                                place.hamlet,
                                { agent: place.number },
                                context,
                            );
                            if (!expired.ok)
                                return refusedTool(toolText(expired.error));
                            if (motion.kind === "found_hamlet") {
                                const refused = await mayFound(
                                    api,
                                    motion,
                                    context,
                                );
                                if (!refused.ok)
                                    return refusedTool(toolText(refused.error));
                            }
                            const called = await memoised(
                                api,
                                {
                                    read: async (tx) => {
                                        const call = await tx.doc(
                                            CallDoc,
                                            api.taskId,
                                        );
                                        return call.id === null
                                            ? undefined
                                            : ok(call.id);
                                    },
                                    change: (tx) =>
                                        callVote(home, tx, {
                                            hamlet: place.hamlet,
                                            by: { agent: place.number },
                                            motion,
                                            reason: args.reason,
                                            conversation: api.conversationId,
                                        }),
                                    write: async (tx, result) => {
                                        if (result.ok)
                                            Object.assign(
                                                await tx.doc(
                                                    CallDoc,
                                                    api.taskId,
                                                ),
                                                {
                                                    hamlet: place.hamlet,
                                                    id: result.value,
                                                },
                                            );
                                    },
                                },
                                context,
                            );
                            if (!called.ok)
                                return refusedTool(toolText(called.error));
                            id = called.value;
                            hamlet = place.hamlet;
                        }
                        return {
                            output: [
                                {
                                    type: "text",
                                    text: await finish(
                                        home,
                                        hamlet,
                                        id,
                                        context,
                                    ),
                                },
                            ],
                        };
                    },
                }),
            ],
        }),
    };
};

async function callerRecord(
    tx: Tx,
    hamlet: string,
    by: Vote["by"],
): Promise<Result<HamletRecord, RecordRefusal>> {
    const r = await tx.doc(RecordDoc, hamlet, hamlet);
    const realm = await tx.doc(RealmDoc);
    if ("chief" in by) {
        if (!isChief(realm, hamlet, by.chief))
            return recordRefusal("call-vote", hamlet, { kind: "vote-chief" });
    } else {
        const number = by.agent;
        if (
            !liveHamlets(realm).some((h) => h.id === hamlet) ||
            !r.agents.some((a) => a.number === number)
        )
            return recordRefusal("call-vote", hamlet, {
                kind: "vote-nonmember",
            });
    }
    return ok(r);
}

async function expire(
    home: Home,
    hamlet: string,
    by: Vote["by"],
    context: Context,
): Promise<Result<void, RecordRefusal>> {
    const runners = await home.harness.commit(async (tx) => {
        const caller = await callerRecord(tx, hamlet, by);
        if (!caller.ok) return caller;
        const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
        const doc = await tx.doc(VotesDoc, hamlet, hamlet);
        const runners: TaskId<string>[] = [];
        for (const v of doc.votes) {
            if (
                v.standing !== "open" ||
                Math.max(0, home.now() - v.called) <
                    limits.votes.openMinutes * 60_000
            )
                continue;
            v.standing = { failed: "lapsed" };
            const key = runKey(hamlet, v.id),
                runner = (await tx.doc(RunDoc, key, key)).runner;
            if (runner !== null) runners.push(runner);
            await tellChiefs(home, tx, hamlet, `Vote ${v.id} failed (lapsed).`);
        }
        return ok(runners);
    }, context);
    if (!runners.ok) return runners;
    for (const runner of runners.value)
        await home.harness.abortTask(runner, context);
    return ok(undefined);
}

async function callVote(
    home: VoteHome,
    tx: Tx,
    given: {
        hamlet: string;
        by: Vote["by"];
        motion: Motion;
        reason: string;
        conversation: ConversationId;
    },
): Promise<Result<number, RecordRefusal>> {
    const caller = await callerRecord(tx, given.hamlet, given.by);
    if (!caller.ok) return caller;
    const r = caller.value;
    if (!given.reason.trim())
        return recordRefusal("call-vote", given.hamlet, {
            kind: "vote-reason",
        });
    const limits = await tx.doc(LimitsDoc, given.hamlet, given.hamlet);
    const doc = await tx.doc(VotesDoc, given.hamlet, given.hamlet);
    for (const open of doc.votes.filter((v) => v.standing === "open")) {
        if (home.now() - open.called < limits.votes.openMinutes * 60_000)
            return recordRefusal("call-vote", given.hamlet, {
                kind: "vote-already-open",
                vote: open.id,
                motion: motionText(open, r),
            });
        open.standing = { failed: "lapsed" };
    }
    const by = JSON.stringify(given.by);
    const retry = doc.votes.findLast(
        (v) =>
            JSON.stringify(v.by) === by &&
            v.motion.kind === given.motion.kind &&
            typeof v.standing === "object" &&
            home.now() - v.called < limits.votes.retryMinutes * 60_000,
    );
    if (retry)
        return recordRefusal("call-vote", given.hamlet, {
            kind: "vote-retry",
            vote: retry.id,
        });
    const voters = r.agents
        .filter((a) => a.number !== r.mayor)
        .map((a) => a.number);
    if (voters.length < limits.votes.leastVoters)
        return recordRefusal("call-vote", given.hamlet, {
            kind: "vote-too-few",
            least: limits.votes.leastVoters,
            actual: voters.length,
        });
    let motion: Vote["motion"];
    if (given.motion.kind === "replace_mayor") {
        const agent = named(r, given.motion.candidate);
        if (agent?.number === r.mayor)
            return recordRefusal("call-vote", given.hamlet, {
                kind: "already-mayor",
                name: given.motion.candidate,
            });
        if (!agent)
            return recordRefusal("call-vote", given.hamlet, {
                kind: "vote-candidate",
                name: given.motion.candidate,
            });
        motion = { kind: "replace_mayor", candidate: agent.number };
    } else motion = given.motion;
    const id = (doc.votes.at(-1)?.id ?? 0) + 1;
    const v: Vote = {
        id,
        motion,
        by: given.by,
        reason: given.reason.trim(),
        mayor: r.mayor,
        voters,
        answer: null,
        ballots: [],
        standing: "open",
        called: home.now(),
        task: newTask(home.now()),
    };
    doc.votes.push(v);
    await tellChiefs(
        home,
        tx,
        given.hamlet,
        `Vote ${id} called by ${byName(v, r)}: ${motionText(v, r)}. ${v.reason}`,
    );
    const task = home[runner];
    if (!task) throw new Error("The vote task is not installed.");
    const key = runKey(given.hamlet, id);
    (await tx.doc(RunDoc, key, key)).runner = await tx.createTask(
        task,
        { hamlet: given.hamlet, id },
        {
            ownership: { kind: "conversation" },
            conversationId: given.conversation,
            background: true,
        },
    );
    return ok(id);
}

/** A chief's vote on the mayor's office. */
export const vote: RecordDeed = async (home, given, context) => {
    const [candidate = "", ...reason] = given.rest.trim().split(/\s+/);
    const r = await home.harness.snapshot(RecordDoc, given.hamlet, context);
    if (!r)
        return recordRefusal("vote", given.hamlet, {
            kind: "vote-hamlet-missing",
        });
    const expired = await expire(
        home,
        given.hamlet,
        { chief: given.chief },
        context,
    );
    if (!expired.ok) return expired;
    const conversation = await current(
        home,
        {
            agent: { hamlet: given.hamlet, number: r.mayor },
            chief: given.chief,
        },
        context,
    );
    const called = await home.harness.commit(
        (tx) =>
            callVote(home, tx, {
                hamlet: given.hamlet,
                by: { chief: given.chief },
                motion: { kind: "replace_mayor", candidate },
                reason: reason.join(" ") || "a chief asked",
                conversation: conversation.id,
            }),
        context,
    );
    if (!called.ok) return called;
    const id = called.value;
    return ok(
        `Vote ${id} is called. Its progress and count will arrive here as notices; use /votes to inspect it or /overturn ${id} to stop it.`,
    );
};
/** The hamlet's votes in words. */
export const votes: RecordDeed = async (home, given, context) =>
    ok(
        (await voteViews(home, given.hamlet, context))
            .map((v) => v.report)
            .join("\n\n") || "This hamlet has held no votes.",
    );
/** A chief's reversal or stop of a vote. */
export const overturn: RecordDeed = async (home, given, context) => {
    const [word = "", ...reason] = given.rest.trim().split(/\s+/),
        id = Number(word);
    const result = await home.harness.commit(async (tx) => {
        if (!isChief(await tx.doc(RealmDoc), given.hamlet, given.chief))
            return recordRefusal("overturn", given.hamlet, {
                kind: "vote-overturn-chief",
            });
        const v = (
            await tx.doc(VotesDoc, given.hamlet, given.hamlet)
        ).votes.find((v) => v.id === id);
        if (!v)
            return recordRefusal("overturn", given.hamlet, {
                kind: "vote-missing",
                vote: word,
            });
        if (v.standing === "overturned")
            return recordRefusal("overturn", given.hamlet, {
                kind: "vote-overturned",
                vote: id,
            });
        if (typeof v.standing === "object")
            return recordRefusal("overturn", given.hamlet, {
                kind: "vote-not-passed",
                vote: id,
            });
        if (v.standing === "open") {
            v.standing = { failed: "stopped by a chief" };
            await tellChiefs(
                home,
                tx,
                given.hamlet,
                `Vote ${id} is stopped by the chief ${given.chief}.`,
            );
            return ok({
                text: `Vote ${id} is stopped.`,
                abort: (
                    await tx.doc(
                        RunDoc,
                        runKey(given.hamlet, id),
                        runKey(given.hamlet, id),
                    )
                ).runner,
            });
        }
        const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
        let done: string;
        if (v.motion.kind === "replace_mayor") {
            if (r.mayor !== v.motion.candidate)
                return recordRefusal("overturn", given.hamlet, {
                    kind: "vote-mayor-changed",
                    vote: id,
                });
            const seated = await seat(home, tx, {
                hamlet: given.hamlet,
                number: v.mayor,
                by: `chief:${given.chief}`,
                reason: reason.join(" ") || "a chief asked",
            });
            if (!seated.ok) return seated;
            done = `\`${nameOf(r, v.mayor)}\` is the mayor again.`;
        } else {
            const removed = await removeHamlet(home, tx, {
                hamlet: v.motion.hamlet,
                by: `chief:${given.chief}`,
                reason: reason.join(" ") || "a chief asked",
            });
            if (!removed.ok) return removed;
            done = `The hamlet \`${v.motion.hamlet}\` is removed and archived.`;
        }
        v.standing = "overturned";
        await tellChiefs(
            home,
            tx,
            given.hamlet,
            `Vote ${id} is overturned by the chief ${given.chief}. ${done}`,
        );
        return ok({ text: `Vote ${id} is overturned. ${done}`, abort: null });
    }, context);
    if (!result.ok) return result;
    if (result.value.abort != null)
        await home.harness.abortTask(result.value.abort, context);
    return ok(result.value.text);
};
/** A chief's direct choice of mayor. */
export const mayor: RecordDeed = async (home, given, context) => {
    const [name = "", ...reason] = given.rest.trim().split(/\s+/);
    return home.harness.commit(async (tx) => {
        if (!isChief(await tx.doc(RealmDoc), given.hamlet, given.chief))
            return recordRefusal("mayor", given.hamlet, {
                kind: "vote-mayor-chief",
            });
        const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet),
            agent = named(r, name);
        if (!agent)
            return recordRefusal("mayor", given.hamlet, {
                kind: "agent-missing",
                name,
            });
        const seated = await seat(home, tx, {
            hamlet: given.hamlet,
            number: agent.number,
            by: `chief:${given.chief}`,
            reason: reason.join(" ") || "a chief asked",
        });
        return seated.ok ? ok(`\`${agent.name}\` is now the mayor.`) : seated;
    }, context);
};
export {
    type DecisionHeld,
    type Vote,
    VotesDoc,
    voteViews,
} from "./record.ts";
