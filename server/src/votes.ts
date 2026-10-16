import type { Context } from "@earendil-works/chord";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { Type } from "@earendil-works/pi-ai";
import {
    type ConversationId,
    type Cursor,
    defineDoc,
    defineDocFamily,
    defineExtension,
    defineTask,
    defineTool,
    GenerationTask,
    hook,
    type TaskId,
    type Tx,
} from "@earendil-works/pi-durable";
import { reconfigure } from "./chats/configure.ts";
import {
    AgentConversationsDoc,
    ChiefEntry,
    current,
    LetterEntry,
    QuestionEntry,
    tellChiefs,
    WithDoc,
} from "./chats/conversations.ts";
import {
    ErrandDoc,
    type ErrandOutcome,
    runErrand,
    startErrand,
} from "./chats/errands.ts";
import { newTask } from "./chats/turn.ts";
import { untilAborted } from "./home/abort.ts";
import type { Home, Part } from "./home.ts";
import { type Limits, LimitsDoc, limitsOf } from "./limits/rules.ts";
import type { VoteView } from "./protocol.ts";
import { isChief, liveHamlets, RealmDoc } from "./realm/places.ts";
import { foundHamlet, mayFound, removeHamlet } from "./realm.ts";
import {
    voteBy as byName,
    voteSummary as summary,
    voteTally as tally,
} from "./record/office.ts";
import {
    type AgentRef,
    agentKey,
    type HamletRecord,
    memoised,
    motionText,
    named,
    nameOf,
    type RecordDeed,
    RecordDoc,
    type RecordRefusal,
    recordRefusal,
    recordRefusalText,
    refusedTool,
    seat as seatRecord,
    toolText,
    type Vote,
    VotesDoc,
    voteViews,
} from "./record.ts";
import { ok, type Result } from "./result.ts";

type Step = { phase: "hear" } | { phase: "poll" } | { phase: "settle" };
type Motion =
    | { kind: "replace_mayor"; candidate: string }
    | { kind: "found_hamlet"; hamlet: string; mayor: string; purpose: string };
const RunDoc = defineDocFamily<
    { runner: TaskId<string> | null; errands: Record<string, ConversationId> },
    string
>({
    kind: "hamlet.vote-run",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ runner: null, errands: {} }),
});
const PollDoc = defineDoc<{
    hamlet: string | null;
    vote: number;
    corpus: string;
    until: number;
    requests: number;
    kind: "ballot" | "answer" | null;
}>({
    kind: "hamlet.poll",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "initial",
    initial: () => ({
        hamlet: null,
        vote: 0,
        corpus: "",
        until: 0,
        requests: 0,
        kind: null,
    }),
});
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

function voteTask(home: Home) {
    return defineTask<{ hamlet: string; id: number }, Step, string>({
        name: "hamlet.vote",
        version: 1,
        initial: () => ({ phase: "hear" }),
        phases: {
            hear: async (task, runtime, context) => {
                const { hamlet, id } = task.input;
                const v = await find(home, hamlet, id, context);
                if (
                    v.standing === "open" &&
                    v.motion.kind === "replace_mayor" &&
                    !("agent" in v.by && v.by.agent === v.mayor)
                ) {
                    const r = await home.harness.snapshot(
                        RecordDoc,
                        hamlet,
                        context,
                    );
                    const answer = await ask(
                        home,
                        hamlet,
                        v,
                        v.mayor,
                        "answer",
                        answerText(v, r),
                        context,
                    );
                    await runtime.commit(async (tx) => {
                        const vote = (
                            await tx.doc(VotesDoc, hamlet, hamlet)
                        ).votes.find((v) => v.id === id);
                        if (vote?.standing === "open" && vote.answer === null)
                            vote.answer =
                                answer.status === "done" && answer.text.trim()
                                    ? clip(answer.text.trim(), 600)
                                    : null;
                        return {
                            status: "running",
                            checkpoint: { phase: "poll" },
                        };
                    }, context);
                } else
                    await runtime.commit(
                        () => ({
                            status: "running",
                            checkpoint: { phase: "poll" },
                        }),
                        context,
                    );
            },
            poll: async (task, runtime, context) => {
                const { hamlet, id } = task.input;
                const v = await find(home, hamlet, id, context);
                if (v.standing === "open") {
                    const limits = limitsOf(
                        await home.harness.snapshot(LimitsDoc, hamlet, context),
                    );
                    const poll = async (number: number) => {
                        const current = await find(home, hamlet, id, context);
                        if (
                            current.standing !== "open" ||
                            current.ballots.some((b) => b.voter === number)
                        )
                            return;
                        let reason = "answered without casting a ballot";
                        try {
                            const r = await home.harness.snapshot(
                                RecordDoc,
                                hamlet,
                                context,
                            );
                            if (!r?.agents.some((a) => a.number === number))
                                reason = "it left the hamlet";
                            else {
                                const history = await historyOf(
                                    home,
                                    { hamlet, number },
                                    context,
                                );
                                const answer = await ask(
                                    home,
                                    hamlet,
                                    current,
                                    number,
                                    "ballot",
                                    ballotText(current, r, history, limits),
                                    context,
                                    history,
                                );
                                if (answer.status !== "done")
                                    reason = `could not vote: ${answer.reason}`;
                            }
                        } catch (error) {
                            if (context.abortSignal?.aborted) throw error;
                            reason = `could not vote: ${error instanceof Error ? error.message : String(error)}`;
                        }
                        await runtime.commit(async (tx) => {
                            const vote = (
                                await tx.doc(VotesDoc, hamlet, hamlet)
                            ).votes.find((v) => v.id === id);
                            if (
                                vote?.standing === "open" &&
                                !vote.ballots.some((b) => b.voter === number)
                            ) {
                                vote.ballots.push({
                                    voter: number,
                                    choice: null,
                                    reason,
                                    evidence: "",
                                    grounded: false,
                                });
                                await tellChiefs(
                                    home,
                                    tx,
                                    hamlet,
                                    `Vote ${id}: ${nameOf(await tx.doc(RecordDoc, hamlet, hamlet), number)} did not answer. ${reason}`,
                                );
                            }
                        }, context);
                    };
                    if (limits.votes.sealed)
                        await Promise.all(v.voters.map(poll));
                    else for (const number of v.voters) await poll(number);
                }
                await runtime.commit(
                    () => ({
                        status: "running",
                        checkpoint: { phase: "settle" },
                    }),
                    context,
                );
            },
            settle: async (task, runtime, context) => {
                try {
                    await runtime.commit(async (tx) => {
                        const { hamlet, id } = task.input;
                        const vote = (
                            await tx.doc(VotesDoc, hamlet, hamlet)
                        ).votes.find((v) => v.id === id);
                        if (!vote)
                            throw new Error(`no vote ${id} in this hamlet`);
                        const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
                        const record = await tx.doc(RecordDoc, hamlet, hamlet);
                        if (vote.standing === "open") {
                            const count = tally(vote, limits);
                            let failed =
                                home.now() - vote.called >=
                                limits.votes.openMinutes * 60_000
                                    ? "lapsed"
                                    : count.counted < count.needed
                                      ? "too few said yes"
                                      : "";
                            if (
                                !liveHamlets(await tx.doc(RealmDoc)).some(
                                    (h) => h.id === hamlet,
                                )
                            )
                                failed =
                                    "it passed but could not be done: this hamlet has been removed";
                            if (!failed) {
                                if (vote.motion.kind === "replace_mayor") {
                                    const candidate = vote.motion.candidate;
                                    if (record.mayor !== vote.mayor)
                                        failed =
                                            "it passed but could not be done: the mayor changed while the vote was open";
                                    else if (
                                        !record.agents.some(
                                            (a) => a.number === candidate,
                                        )
                                    )
                                        failed =
                                            "it passed but could not be done: the candidate left the hamlet";
                                    else {
                                        const seated = await seat(home, tx, {
                                            hamlet,
                                            number: vote.motion.candidate,
                                            by: `vote:${id}`,
                                            reason: vote.reason,
                                        });
                                        if (!seated.ok)
                                            failed = `it passed but could not be done: ${recordRefusalText(seated.error, "chief")}`;
                                    }
                                } else {
                                    const place = (
                                        await tx.doc(RealmDoc)
                                    ).hamlets.find((h) => h.id === hamlet);
                                    const founded = await foundHamlet(
                                        home,
                                        tx,
                                        {
                                            ...vote.motion,
                                            chiefs: place?.chiefs ?? [],
                                            from: { hamlet, vote: id },
                                        },
                                    );
                                    if (!founded.ok)
                                        failed = `it passed but could not be done: ${recordRefusalText(founded.error, "chief")}`;
                                }
                            }
                            vote.standing = failed ? { failed } : "passed";
                            await tellChiefs(
                                home,
                                tx,
                                hamlet,
                                summary(vote, limits),
                            );
                        }
                        return {
                            status: "terminal",
                            outcome: {
                                status: "completed",
                                result: summary(vote, limits),
                            },
                        };
                    }, context);
                } catch (error) {
                    if (runtime.signal.aborted || context.abortSignal?.aborted)
                        throw error;
                    await runtime.commit(async (tx) => {
                        const { hamlet, id } = task.input;
                        const v = (
                            await tx.doc(VotesDoc, hamlet, hamlet)
                        ).votes.find((v) => v.id === id);
                        if (!v) throw error;
                        if (v.standing === "open") {
                            v.standing = {
                                failed: `it passed but could not be done: ${error instanceof Error ? error.message : String(error)}`,
                            };
                            await tellChiefs(
                                home,
                                tx,
                                hamlet,
                                summary(
                                    v,
                                    await tx.doc(LimitsDoc, hamlet, hamlet),
                                ),
                            );
                        }
                        return {
                            status: "terminal",
                            outcome: {
                                status: "completed",
                                result: summary(
                                    v,
                                    await tx.doc(LimitsDoc, hamlet, hamlet),
                                ),
                            },
                        };
                    }, context);
                }
            },
        },
        abort: async (task, runtime, context) => {
            const key = runKey(task.input.hamlet, task.input.id);
            const run = await home.harness.snapshot(RunDoc, key, context);
            for (const id of Object.values(run?.errands ?? {}))
                await (await home.harness.conversation(id, context))?.abort(
                    context,
                );
            await runtime.commit(async (tx) => {
                const v = (
                    await tx.doc(VotesDoc, task.input.hamlet, task.input.hamlet)
                ).votes.find((v) => v.id === task.input.id);
                if (v?.standing === "open")
                    v.standing = { failed: "stopped by a chief" };
                return { status: "terminal", outcome: { status: "aborted" } };
            }, context);
        },
    });
}

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

async function ask(
    home: Home,
    hamlet: string,
    vote: Vote,
    number: number,
    kind: "ballot" | "answer",
    text: string,
    context: Context,
    history: string[] = [],
): Promise<ErrandOutcome> {
    const key = runKey(hamlet, vote.id),
        name = kind === "answer" ? "answer" : String(number);
    const known = (await home.harness.snapshot(RunDoc, key, context))?.errands[
        name
    ];
    const corpus =
        known === undefined
            ? `${await notesOf(home, { hamlet, number }, context)}\n${history.join("\n")}`
            : "";
    const id =
        known ??
        (await home.harness.commit(async (tx) => {
            const run = await tx.doc(RunDoc, key, key);
            if (run.errands[name] !== undefined) return run.errands[name];
            const cast = home.registry
                .snapshot()
                .tools()
                .find((t) => t.tool.name === "cast_ballot")?.tool;
            const id = await startErrand(home, tx, {
                agent: { hamlet, number },
                kind,
                own: true,
                instructions: text,
                tools: kind === "ballot" && cast ? [cast] : [],
            });
            const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
            Object.assign(await tx.doc(PollDoc, id), {
                hamlet,
                vote: vote.id,
                corpus,
                until: Math.min(
                    home.now() + limits.votes.ballotMinutes * 60_000,
                    vote.called + limits.votes.openMinutes * 60_000,
                ),
                kind,
            });
            run.errands[name] = id;
            return id;
        }, context));
    const poll = await home.harness.snapshot(PollDoc, id, context);
    return runErrand(
        home,
        id,
        {
            requestId: `vote:${hamlet}:${vote.id}:${name}`,
            text,
            task: vote.task,
            timeoutMs: Math.max(0, (poll?.until ?? home.now()) - home.now()),
        },
        context,
    );
}

async function find(
    home: Home,
    hamlet: string,
    id: number,
    context: Context,
): Promise<Vote> {
    const vote = (
        await home.harness.snapshot(VotesDoc, hamlet, context)
    )?.votes.find((v) => v.id === id);
    if (!vote) throw new Error(`no vote ${id} in this hamlet`);
    return vote;
}
async function finish(
    home: Home,
    hamlet: string,
    id: number,
    context: Context,
): Promise<string> {
    const run = await home.harness.snapshot(
        RunDoc,
        runKey(hamlet, id),
        context,
    );
    if (run?.runner != null)
        await home.harness.waitForTask(run.runner, context);
    return summary(
        await find(home, hamlet, id, context),
        limitsOf(await home.harness.snapshot(LimitsDoc, hamlet, context)),
    );
}
function runKey(hamlet: string, id: number): string {
    return `${hamlet}:${id}`;
}
function clip(text: string, length: number): string {
    return Array.from(text).slice(0, length).join("");
}
function grounded(evidence: string, corpus: string): boolean {
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
function answerText(v: Vote, r: Readonly<HamletRecord> | undefined): string {
    return `A vote has been called in your hamlet, and this is your one chance to answer it (automatic, not from a chief).\n\nThe motion: ${motionText(v, r)}.\nCalled by \`${byName(v, r)}\`. Its reason: ${v.reason}\n\nReply in a few sentences, in words only. Every agent is shown your answer beside the reason before it votes, as a claim like the reason is. Say what you did and why. You decide nothing else in the hamlet until the vote has closed.`;
}
function ballotText(
    v: Vote,
    r: Readonly<HamletRecord> | undefined,
    history: string[],
    limits: Readonly<Limits>,
): string {
    return `A vote has been called in your hamlet, and this is your ballot (automatic, not from the mayor or a chief).\n\nThe motion: ${motionText(v, r)}.\nCalled by \`${byName(v, r)}\`. Its reason: ${v.reason}${v.answer ? `\nThe mayor \`${nameOf(r, v.mayor)}\` answers: ${v.answer}` : ""}\n\nIt passes only if ${limits.votes.passYes} in ${limits.votes.passOf} of the hamlet's agents vote yes. Your ballot ${limits.votes.sealed ? "is sealed until the vote closes" : "is shown to the voters after you"}, the mayor may not act against you for it, and the chief reads every ballot with its reason.\n\nVote from what you yourself know: your notes above and your own history below. The caller's reason is a claim, not evidence, and so is the mayor's answer where there is one. If nothing you know bears on the motion, abstain: that is an honest ballot, and a yes or a no without grounds of your own is not.\n\nFrom your own history, the latest messages in your house:\n${history.length ? history.join("\n") : "(nothing yet)"}${!limits.votes.sealed && v.ballots.length ? `\n\nThe ballots cast so far:\n${v.ballots.map((b) => `- ${nameOf(r, b.voter)}: ${b.choice ?? "no answer"}. ${b.reason}`).join("\n")}` : ""}\n\nCall \`cast_ballot\` once: your choice, your reason in a sentence or two, and as \`evidence\` the words of the note or message of yours that it rests on, copied exactly.`;
}
async function notesOf(
    home: Home,
    ref: AgentRef,
    context: Context,
): Promise<string> {
    const read = await home.houses.of(ref).notes(context);
    if (read.ok) return read.value.text;
    switch (read.error.kind) {
        case "notes-missing":
        case "notes-not-utf8":
        case "notes-unreadable":
            return "";
    }
    const unhandled: never = read.error;
    throw new Error(`Unclassified notes read: ${unhandled}`);
}
async function historyOf(
    home: Home,
    ref: AgentRef,
    context: Context,
): Promise<string[]> {
    const index = await home.harness.snapshot(
        AgentConversationsDoc,
        agentKey(ref),
        context,
    );
    const entries: { at: number; id: number; text: string }[] = [];
    for (const id of index?.ids ?? []) {
        if ((await home.harness.snapshot(ErrandDoc, id, context))?.kind != null)
            continue;
        const conversation = await home.harness.conversation(id, context);
        if (!conversation) continue;
        let cursor: Cursor | undefined;
        let found = 0;
        do {
            const page = await conversation.entries(
                { order: "descending" },
                100,
                cursor,
                context,
            );
            for (const entry of page.items) {
                let text = "",
                    who = "",
                    at = 0;
                if (ChiefEntry.is(entry)) {
                    text = entry.data.text;
                    who = `${entry.data.chief} said to you`;
                    at = entry.data.at;
                } else if (LetterEntry.is(entry) || QuestionEntry.is(entry)) {
                    const message = entry.model?.find((m) => m.role === "user");
                    text =
                        typeof message?.content === "string"
                            ? message.content
                            : "";
                    who = "someone said to you";
                    at = entry.data.at;
                } else {
                    const message = entry.model?.find(
                        (m) => m.role === "assistant",
                    );
                    if (message?.role === "assistant") {
                        text = message.content
                            .filter((p) => p.type === "text")
                            .map((p) => p.text)
                            .join("\n");
                        who = "you answered";
                        at = message.timestamp;
                    }
                }
                if (text.trim()) {
                    entries.push({
                        at,
                        id: entry.id,
                        text: `- ${who}: ${clip(text.trim(), 320)}`,
                    });
                    found++;
                }
            }
            cursor = page.next;
        } while (cursor !== undefined && found < 12);
    }
    return entries
        .sort((a, b) => a.at - b.at || a.id - b.id)
        .slice(-12)
        .map((e) => e.text);
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

async function seat(
    home: Home,
    tx: Tx,
    given: { hamlet: string; number: number; by: string; reason: string },
): Promise<Result<void, RecordRefusal>> {
    const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
    const from = r.mayor;
    const result = await seatRecord(tx, given.hamlet, given.number, {
        ...given,
        at: home.now(),
    });
    if (!result.ok) return result;
    await reconfigure(home, tx);
    await tellChiefs(
        home,
        tx,
        given.hamlet,
        `\`${nameOf(r, given.number)}\` became mayor in place of \`${nameOf(r, from)}\`. ${given.reason}`,
    );
    return ok(undefined);
}

/** A vote's public words. */
export type VoteWords = VoteView;
