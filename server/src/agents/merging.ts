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
    ToolTask,
    type Tx,
} from "@earendil-works/pi-durable";
import { WithDoc } from "../chats/conversations.ts";
import { ErrandDoc, runErrand, startErrand } from "../chats/errands.ts";
import { newTask, turnOf } from "../chats/turn.ts";
import { stopAgent } from "../chats.ts";
import { untilAborted } from "../home/abort.ts";
import type { Home, Part } from "../home.ts";
import { LimitsDoc } from "../limits/rules.ts";
import { chiefAccess, chiefRefusalText } from "../realm/access.ts";
import { isChief, liveHamlets, RealmDoc } from "../realm/places.ts";
import {
    type AgentRecord,
    type Archived,
    agentKey,
    copyArchive,
    type DecisionHeld,
    depart,
    mayDecide,
    memoised,
    mergeable,
    named,
    type RecordDeed,
    RecordDoc,
    type RecordRefusal,
    recordRefusal,
    refusedTool,
    requireMayor,
    restore as restoreRecord,
    toolText,
} from "../record.ts";
import { err, ok, type Result } from "../result.ts";
import { canSpend, spendFailureText } from "../spend.ts";
import {
    pendingText,
    prepare,
    prepareText,
    retryFiles,
} from "./merge-files.ts";
import { accept, digest, mergedParagraph, type Planned } from "./merge-plan.ts";

type Given = {
    hamlet: string;
    keep: string;
    absorb: string;
    purpose: string | null;
    reason: string;
    by: string;
};

const MergeCallDoc = defineDoc<{
    errand: ConversationId | null;
    archive: string | null;
    text: string;
}>({
    kind: "hamlet.merge-call",
    version: 1,
    scope: "task",
    initial: () => ({ errand: null, archive: null, text: "" }),
});

const HeldDoc = defineDoc<{ agents: string[] }>({
    kind: "hamlet.merge-held",
    version: 1,
    scope: "session",
    initial: () => ({ agents: [] }),
});

async function stoppedHouses<T>(
    home: Home,
    hamlet: string,
    agents: AgentRecord[],
    context: Context,
    action: () => Promise<T>,
): Promise<T> {
    const added = await home.harness.commit(async (tx) => {
        const held = await tx.doc(HeldDoc);
        const added = agents
            .map((a) => agentKey({ hamlet, number: a.number }))
            .filter((key) => !held.agents.includes(key));
        held.agents.push(...added);
        return added;
    }, context);
    try {
        for (const agent of agents)
            await stopAgent(home, { hamlet, number: agent.number }, context);
        return await action();
    } finally {
        await home.harness.commit(async (tx) => {
            const held = await tx.doc(HeldDoc);
            held.agents = held.agents.filter((key) => !added.includes(key));
        }, BACKGROUND_CONTEXT);
    }
}

const operations = Symbol("house work");

/** Serializes merge, split and erasure file work within one home. */
export async function serialHouseWork<T>(
    home: Home & { [operations]?: Promise<unknown> },
    context: Context,
    action: () => Promise<T>,
): Promise<T> {
    const run = (home[operations] ?? Promise.resolve())
        .catch(() => {})
        .then(() => {
            context.abortSignal?.throwIfAborted();
            return action();
        });
    home[operations] = run;
    try {
        return await run;
    } finally {
        if (home[operations] === run) delete home[operations];
    }
}

/** Merges and their reversal. */
export const Merging: Part = (home) => ({
    extension: defineExtension({
        name: "hamlet.merging",
        hooks: [
            hook(GenerationTask, {
                beforeRequest: async (_request, api, context) => {
                    if (
                        (
                            await api.snapshot(
                                ErrandDoc,
                                api.conversationId,
                                context,
                            )
                        )?.kind === "ballot"
                    )
                        return undefined;
                    const place = await api.snapshot(
                        WithDoc,
                        api.conversationId,
                        context,
                    );
                    if (
                        place &&
                        (await api.snapshot(HeldDoc, context))?.agents.includes(
                            agentKey(place),
                        )
                    ) {
                        void home.harness.abortTask(
                            api.taskId,
                            BACKGROUND_CONTEXT,
                        );
                        return untilAborted(context);
                    }
                },
            }),
            hook(ToolTask, {
                beforeTool: async (_call, api, context) => {
                    if (
                        (
                            await api.snapshot(
                                ErrandDoc,
                                api.conversationId,
                                context,
                            )
                        )?.kind === "ballot"
                    )
                        return undefined;
                    const place = await api.snapshot(
                        WithDoc,
                        api.conversationId,
                        context,
                    );
                    if (
                        place &&
                        (await api.snapshot(HeldDoc, context))?.agents.includes(
                            agentKey(place),
                        )
                    )
                        return {
                            block: "This house is being merged or split. Wait for its files to finish, then ask the chief to send a new instruction.",
                        };
                    return undefined;
                },
            }),
        ],
        tools: [
            defineTool({
                name: "merge_agents",
                description:
                    "Merge two agents' houses and roads under the kept agent's new purpose. A reason is kept in the record.",
                parameters: Type.Object({
                    keep: Type.String(),
                    absorb: Type.String(),
                    purpose: Type.String(),
                    reason: Type.String(),
                }),
                replay: "safe",
                execute: async (args, api, context) =>
                    serialHouseWork(home, context, async () => {
                        const place = await api.snapshot(
                            WithDoc,
                            api.conversationId,
                            context,
                        );
                        if (!place)
                            return refusedTool(
                                "This conversation has no agent.",
                            );
                        const saved = await api.snapshot(
                            MergeCallDoc,
                            api.taskId,
                            context,
                        );
                        if (saved?.archive) {
                            const entry = (
                                await api.snapshot(
                                    RecordDoc,
                                    place.hamlet,
                                    context,
                                )
                            )?.archive.find((a) => a.id === saved.archive);
                            if (entry) {
                                const outcome = await retryFiles(
                                    home,
                                    place.hamlet,
                                    entry,
                                    context,
                                    stoppedHouses,
                                );
                                if (!outcome.ok)
                                    return {
                                        output: [
                                            {
                                                type: "text",
                                                text: pendingText(
                                                    outcome.error,
                                                ),
                                            },
                                        ],
                                    };
                            }
                            return {
                                output: [{ type: "text", text: saved.text }],
                            };
                        }
                        const given: Given = {
                            ...args,
                            hamlet: place.hamlet,
                            by: "mayor",
                        };
                        const checked = await api.commit(async (tx) => {
                            const held = await mayDecide(
                                tx,
                                place.hamlet,
                                home.now(),
                                "merge",
                            );
                            if (!held.ok) return held;
                            const mayor = await requireMayor(tx, place);
                            if (!mayor.ok)
                                return recordRefusal("merge", place.hamlet, {
                                    kind: "not-mayor",
                                });
                            return mergeable(tx, given.hamlet, {
                                ...given,
                                at: home.now(),
                            });
                        }, context);
                        if (!checked.ok)
                            return checked.error.kind === "vote-open"
                                ? {
                                      output: [
                                          {
                                              type: "text",
                                              text: toolText(checked.error),
                                          },
                                      ],
                                  }
                                : refusedTool(toolText(checked.error));
                        const agents = checked.value;
                        return stoppedHouses(
                            home,
                            place.hamlet,
                            [agents.kept, agents.gone],
                            context,
                            async () => {
                                const prepared = await prepare(
                                    home,
                                    place.hamlet,
                                    agents,
                                    context,
                                );
                                if (!prepared.ok)
                                    return prepared.error.kind ===
                                        "notes-unreadable" ||
                                        prepared.error.kind ===
                                            "destination-blocked"
                                        ? {
                                              output: [
                                                  {
                                                      type: "text",
                                                      text: prepareText(
                                                          prepared.error,
                                                      ),
                                                  },
                                              ],
                                          }
                                        : refusedTool(
                                              prepareText(prepared.error),
                                          );
                                const plan = prepared.value;
                                let why = "";
                                if (plan.notes === "joined") {
                                    const limits = await api.snapshot(
                                        LimitsDoc,
                                        place.hamlet,
                                        context,
                                    );
                                    if (!limits)
                                        throw new Error(
                                            "The hamlet has no limits.",
                                        );
                                    const turn = await turnOf(
                                        home.harness,
                                        api,
                                        context,
                                    );
                                    const task =
                                        turn?.task ?? newTask(home.now());
                                    const allowed = await canSpend(
                                        home,
                                        api,
                                        { task },
                                        context,
                                    );
                                    why = allowed.ok
                                        ? ""
                                        : spendFailureText(allowed.error);
                                    if (!why) {
                                        const errand = await memoised(
                                            api,
                                            {
                                                read: async (tx) =>
                                                    (
                                                        await tx.doc(
                                                            MergeCallDoc,
                                                            api.taskId,
                                                        )
                                                    ).errand ?? undefined,
                                                change: (tx) =>
                                                    startErrand(home, tx, {
                                                        agent: place,
                                                        kind: "notes",
                                                        own: false,
                                                        instructions:
                                                            mergeRequest(
                                                                agents.kept,
                                                                agents.gone,
                                                                args.purpose,
                                                                limits.leaving
                                                                    .mergedNotes,
                                                            ),
                                                        tools: [],
                                                    }),
                                                write: async (tx, value) => {
                                                    (
                                                        await tx.doc(
                                                            MergeCallDoc,
                                                            api.taskId,
                                                        )
                                                    ).errand = value;
                                                },
                                            },
                                            context,
                                        );
                                        const answer = await runErrand(
                                            home,
                                            errand,
                                            {
                                                requestId: `merge-notes:${api.taskId}`,
                                                text: `Your NOTES.md now:\n<notes of="${args.keep}">\n${plan.before}\n</notes>\n\nThe NOTES.md of \`${args.absorb}\`:\n<notes of="${args.absorb}">\n${plan.files.get(`merged/${args.absorb}/NOTES.md`)?.toString("utf8") ?? ""}\n</notes>`,
                                                task,
                                            },
                                            context,
                                        );
                                        const rewritten =
                                            answer.status === "done"
                                                ? accept(
                                                      answer.text,
                                                      limits.leaving
                                                          .mergedNotes,
                                                  )
                                                : undefined;
                                        if (rewritten) {
                                            plan.after = `${rewritten}\n\n${mergedParagraph(agents.gone)}`;
                                            plan.notes = "rewritten";
                                        } else
                                            why =
                                                answer.status === "done"
                                                    ? "The reply could not be used as merged notes."
                                                    : answer.reason;
                                    }
                                }
                                const committed = await memoised(
                                    api,
                                    {
                                        read: async (
                                            tx,
                                        ): Promise<
                                            | Result<
                                                  Archived,
                                                  RecordRefusal | DecisionHeld
                                              >
                                            | undefined
                                        > => {
                                            const call = await tx.doc(
                                                MergeCallDoc,
                                                api.taskId,
                                            );
                                            if (!call.archive) return undefined;
                                            const entry = (
                                                await tx.doc(
                                                    RecordDoc,
                                                    place.hamlet,
                                                    place.hamlet,
                                                )
                                            ).archive.find(
                                                (a) => a.id === call.archive,
                                            );
                                            if (!entry)
                                                throw new Error(
                                                    "A merge memo lost its archive.",
                                                );
                                            return ok(copyArchive(entry));
                                        },
                                        change: async (tx) => {
                                            const decision = await mayDecide(
                                                tx,
                                                place.hamlet,
                                                home.now(),
                                                "merge",
                                            );
                                            if (!decision.ok) return decision;
                                            const mayor = await requireMayor(
                                                tx,
                                                place,
                                            );
                                            if (!mayor.ok)
                                                return recordRefusal(
                                                    "merge",
                                                    place.hamlet,
                                                    { kind: "not-mayor" },
                                                );
                                            return archive(
                                                home,
                                                tx,
                                                given,
                                                agents,
                                                plan,
                                            );
                                        },
                                        write: async (tx, result) => {
                                            if (!result.ok) return;
                                            const call = await tx.doc(
                                                MergeCallDoc,
                                                api.taskId,
                                            );
                                            call.archive = result.value.id;
                                            call.text =
                                                report(
                                                    result.value,
                                                    plan.notes === "joined",
                                                    args.purpose,
                                                ) + (why ? ` ${why}` : "");
                                        },
                                    },
                                    context,
                                );
                                if (!committed.ok)
                                    return committed.error.kind === "vote-open"
                                        ? {
                                              output: [
                                                  {
                                                      type: "text",
                                                      text: toolText(
                                                          committed.error,
                                                      ),
                                                  },
                                              ],
                                          }
                                        : refusedTool(
                                              toolText(committed.error),
                                          );
                                const entry = committed.value;
                                const outcome = await retryFiles(
                                    home,
                                    place.hamlet,
                                    entry,
                                    context,
                                    stoppedHouses,
                                );
                                if (!outcome.ok)
                                    return {
                                        output: [
                                            {
                                                type: "text",
                                                text: pendingText(
                                                    outcome.error,
                                                ),
                                            },
                                        ],
                                    };
                                const result = await api.snapshot(
                                    MergeCallDoc,
                                    api.taskId,
                                    context,
                                );
                                return {
                                    output: [
                                        {
                                            type: "text",
                                            text:
                                                result?.text ??
                                                report(
                                                    entry,
                                                    plan.notes === "joined",
                                                    args.purpose,
                                                ),
                                        },
                                    ],
                                };
                            },
                        );
                    }),
            }),
        ],
    }),
    async start(context) {
        await home.harness.commit(async (tx) => {
            (await tx.doc(HeldDoc)).agents = [];
        }, context);
        const realm = await home.harness.snapshot(RealmDoc, context);
        for (const { id } of liveHamlets(realm)) {
            const record = await home.harness.snapshot(RecordDoc, id, context);
            for (const entry of record?.archive ?? [])
                if (entry.merge && !entry.merge.applied)
                    await retryFiles(home, id, entry, context, stoppedHouses);
        }
    },
});

/** A chief's merge deed. */
export const merge: RecordDeed = async (home, given, context) =>
    serialHouseWork(home, context, async () => {
        const [absorb = "", into, keep = "", ...reason] = given.rest
            .trim()
            .split(/\s+/);
        if (into !== "into")
            return recordRefusal("merge", given.hamlet, {
                kind: "record-usage",
                command: "merge",
            });
        const args: Given = {
            hamlet: given.hamlet,
            keep,
            absorb,
            purpose: null,
            reason: reason.join(" ") || "a chief asked",
            by: `chief:${given.chief}`,
        };
        const access = await chiefAccess(
            home,
            given.hamlet,
            "merge",
            context,
            given.chief,
        );
        if (!access.ok) return ok(chiefRefusalText(access));
        const record = await home.harness.snapshot(
            RecordDoc,
            given.hamlet,
            context,
        );
        const pending = record?.archive.findLast(
            (entry) =>
                entry.back === null &&
                entry.merge &&
                (entry.id === absorb ||
                    (entry.agent.name === absorb &&
                        record &&
                        !named(record, absorb))),
        );
        if (
            record &&
            pending?.merge &&
            named(record, keep)?.number === pending.merge.kept.number
        ) {
            const outcome = await retryFiles(
                home,
                given.hamlet,
                pending,
                context,
                stoppedHouses,
            );
            if (!outcome.ok) return ok(pendingText(outcome.error));
            const kept = record.agents.find(
                (agent) => agent.number === pending.merge?.kept.number,
            );
            return ok(
                report(
                    pending,
                    false,
                    kept?.purpose ?? pending.merge.kept.purpose,
                ),
            );
        }
        const checked = await home.harness.commit(
            (tx) => mergeable(tx, args.hamlet, { ...args, at: home.now() }),
            context,
        );
        if (!checked.ok) return checked;
        const agents = checked.value;
        return stoppedHouses(
            home,
            args.hamlet,
            [agents.kept, agents.gone],
            context,
            async () => {
                const prepared = await prepare(
                    home,
                    args.hamlet,
                    agents,
                    context,
                );
                if (!prepared.ok) {
                    const failure = prepared.error;
                    return failure.kind === "notes-unreadable" ||
                        failure.kind === "destination-blocked"
                        ? ok(prepareText(failure))
                        : err(failure);
                }
                const plan = prepared.value;
                const archived = await home.harness.commit(
                    (tx) => archive(home, tx, args, agents, plan),
                    context,
                );
                if (!archived.ok) return archived;
                const entry = archived.value;
                const outcome = await retryFiles(
                    home,
                    args.hamlet,
                    entry,
                    context,
                    stoppedHouses,
                );
                if (!outcome.ok) return ok(pendingText(outcome.error));
                return ok(
                    report(entry, plan.notes === "joined", agents.kept.purpose),
                );
            },
        );
    });

async function archive(
    home: Home,
    tx: Tx,
    given: Given,
    was: { kept: AgentRecord; gone: AgentRecord },
    plan: Planned,
): Promise<Result<Archived, RecordRefusal>> {
    const allowedNow = await mergeable(tx, given.hamlet, {
        ...given,
        at: home.now(),
    });
    if (!allowedNow.ok) return allowedNow;
    const { kept, gone } = allowedNow.value;
    if (kept.number !== was.kept.number || gone.number !== was.gone.number)
        return recordRefusal("merge", given.hamlet, { kind: "merge-changed" });
    return depart(tx, given.hamlet, given.absorb, {
        by: given.by,
        reason: given.reason,
        how: "merged",
        into: kept.number,
        purpose: given.purpose,
        notes: plan.notes,
        at: home.now(),
        merge: {
            notesBefore: plan.before,
            notesAfter: plan.after,
            brought: [...plan.files].map(([path, bytes]) => ({
                path,
                sha256: digest(bytes),
            })),
            clashes: plan.clashes,
        },
    });
}

function mergeRequest(
    kept: AgentRecord,
    gone: AgentRecord,
    purpose: string,
    limit: number,
): string {
    return `You are \`${kept.name}\`, an agent in a hamlet of agents. The mayor is merging the agent \`${gone.name}\` into you: from now on you do the work of both, under your own name. Your purpose was: ${kept.purpose} Its purpose was: ${gone.purpose} Your purpose becomes: ${purpose.trim()}\nYou are writing your new NOTES.md, the memory that is shown to you on every turn.\nWrite the one NOTES.md you will keep.\n- Keep every fact, preference and lesson from both. Drop nothing because it looks minor.\n- Where both say the same thing, say it once.\n- Where they disagree, keep both and say which of you held which.\n- Speak as yourself: things \`${gone.name}\` wrote as "I" are now things you know or do.\n- Group it by subject, not by who wrote it.\n- At most ${limit} characters.\n- Start with the line \`# Notes\`. Reply with the file's text only: no code fence, no comment before or after.`;
}

function report(entry: Archived, joined: boolean, purpose: string): string {
    const m = entry.merge;
    return `Merged \`${entry.agent.name}\` into \`${m?.kept.name}\`. \`${m?.kept.name}\` has the notes, skills, files and roads of both, and its purpose is now: ${purpose} \`${entry.agent.name}\` is archived as ${entry.id}.${joined ? ` The two notes were joined, not rewritten: ask \`${m?.kept.name}\` to tidy its NOTES.md.` : ""}${m?.clashes.length ? ` Kept apart because they differed: ${m.clashes.join("; ")}.` : ""}`;
}

/** A merged agent split back out of its kept house. */
export async function split(
    home: Home,
    given: {
        hamlet: string;
        entry: Archived;
        name: string;
        reason: string;
        chief: string;
    },
    context: Context,
): Promise<Result<string, RecordRefusal>> {
    return serialHouseWork(home, context, async () => {
        if (given.entry.merge && !given.entry.merge.applied) {
            const outcome = await retryFiles(
                home,
                given.hamlet,
                given.entry,
                context,
                stoppedHouses,
            );
            if (!outcome.ok) return ok(pendingText(outcome.error));
            if (given.entry.back !== null)
                return ok(
                    `Split of \`${given.entry.agent.name}\` is finished. Read /record to inspect it.`,
                );
        }
        return splitOnce(home, given, context);
    });
}

async function splitOnce(
    home: Home,
    given: {
        hamlet: string;
        entry: Archived;
        name: string;
        reason: string;
        chief: string;
    },
    context: Context,
): Promise<Result<string, RecordRefusal>> {
    const pending = await home.harness.commit(async (tx) => {
        if (!isChief(await tx.doc(RealmDoc), given.hamlet, given.chief))
            return recordRefusal("restore", given.hamlet, {
                kind: "record-chief",
            });
        const restored = await restoreRecord(tx, given.hamlet, given.entry.id, {
            name: given.name,
            reason: given.reason,
            by: `chief:${given.chief}`,
            at: home.now(),
        });
        if (!restored.ok) return restored;
        const entry = restored.value.archive;
        const kept = entry.merge?.kept;
        if (!kept)
            throw new Error("A split requires its checked merge manifest.");
        return ok({ entry: copyArchive(entry), words: restored.value.text });
    }, context);
    if (!pending.ok) return pending;
    const outcome = await retryFiles(
        home,
        given.hamlet,
        pending.value.entry,
        context,
        stoppedHouses,
    );
    return ok(outcome.ok ? pending.value.words : pendingText(outcome.error));
}
