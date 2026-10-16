import { isUtf8 } from "node:buffer";
import { createHash } from "node:crypto";
import {
    lstat,
    mkdir,
    mkdtemp,
    readFile,
    rm,
    writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
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
import { tellChiefs, WithDoc } from "../chats/conversations.ts";
import { ErrandDoc, runErrand, startErrand } from "../chats/errands.ts";
import { newTask, turnOf } from "../chats/turn.ts";
import { stopAgent } from "../chats.ts";
import { houseReader, treeWorker } from "../environment.ts";
import { untilAborted } from "../home/abort.ts";
import { houseOf } from "../home/layout.ts";
import type { Home, Part } from "../home.ts";
import { listTree, MERGE_CAPS, removeTree, stageTree } from "../house.ts";
import { LimitsDoc } from "../limits/rules.ts";
import { chiefAccess, chiefRefusalText } from "../realm/access.ts";
import { isChief, liveHamlets, RealmDoc } from "../realm/places.ts";
import {
    type AgentRecord,
    type AgentRef,
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
import type { SandboxEnv } from "../sandbox.ts";
import { safeWords } from "../scrub.ts";
import { booksOf } from "../spend/ledger.ts";
import { stopped } from "../spend.ts";

type Planned = {
    files: Map<string, Buffer>;
    clashes: string[];
    before: string;
    after: string;
    notes: "joined" | "rewritten" | "plain";
};
type NotesFailure = {
    kind: "notes-unreadable";
    operation: "merge" | "split";
    agent: AgentRef;
    reason: "not-utf8" | "unavailable";
    retrySafe: true;
};
type PrepareFailure =
    | {
          kind: "destination-blocked";
          operation: "merge";
          path: string;
          retrySafe: true;
      }
    | NotesFailure
    | RecordRefusal;
type Prepared = Result<Planned, PrepareFailure>;

function prepareText(error: PrepareFailure): string {
    switch (error.kind) {
        case "notes-unreadable":
            return notesFailureText(error);
        case "destination-blocked":
            return `The merge cannot safely deliver \`${error.path}\`; keep its destination and any linked parent aside before merging.`;
    }
    return toolText(error);
}

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
                                    why =
                                        stopped(
                                            limits,
                                            await booksOf(
                                                home,
                                                place.hamlet,
                                                limits.budget.period,
                                                task,
                                                context,
                                            ),
                                        ) ?? "";
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
                                                    : answer.text;
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
                    await retryFiles(home, id, entry, context);
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
                );
                if (!outcome.ok) return ok(pendingText(outcome.error));
                return ok(
                    report(entry, plan.notes === "joined", agents.kept.purpose),
                );
            },
        );
    });

function envFor(home: Home, ref: AgentRef, write: boolean): SandboxEnv {
    return write
        ? treeWorker(home, {
              root: houseOf(home.layout, ref),
              stage: houseOf(home.layout, ref),
              caps: MERGE_CAPS,
          })
        : houseReader(home, ref, BACKGROUND_CONTEXT);
}

async function notes(
    home: Home,
    ref: AgentRef,
    context: Context,
    operation: "merge" | "split" = "merge",
): Promise<Result<{ text: string; bytes: Buffer }, NotesFailure>> {
    const env = envFor(home, ref, false);
    try {
        const read = await env.readBinaryFile(
            join(houseOf(home.layout, ref), "NOTES.md"),
            context,
        );
        if (!read.ok) {
            if (read.error.code === "not_found")
                return {
                    ok: true,
                    value: {
                        text: "# Notes\n",
                        bytes: Buffer.from("# Notes\n"),
                    },
                };
            return {
                ok: false,
                error: {
                    kind: "notes-unreadable",
                    operation,
                    agent: ref,
                    reason: "unavailable",
                    retrySafe: true,
                },
            };
        }
        if (!isUtf8(read.value))
            return {
                ok: false,
                error: {
                    kind: "notes-unreadable",
                    operation,
                    agent: ref,
                    reason: "not-utf8",
                    retrySafe: true,
                },
            };
        return {
            ok: true,
            value: {
                text: Buffer.from(read.value).toString("utf8"),
                bytes: Buffer.from(read.value),
            },
        };
    } finally {
        await env.cleanup(context);
    }
}

function substance(text: string): string {
    return text
        .split("\n")
        .filter((line) => line.trim() !== "# Notes")
        .join("\n")
        .trim();
}
function digest(bytes: Buffer | string): string {
    return createHash("sha256").update(bytes).digest("hex");
}

async function prepare(
    home: Home,
    hamlet: string,
    { kept, gone }: { kept: AgentRecord; gone: AgentRecord },
    context: Context,
): Promise<Prepared> {
    const source = houseOf(home.layout, { hamlet, number: gone.number });
    const target = houseOf(home.layout, { hamlet, number: kept.number });
    const stage = await mkdtemp(join(tmpdir(), "hamlet-merge-"));
    try {
        try {
            await stageTree(
                home,
                { root: source, rel: "." },
                stage,
                MERGE_CAPS,
                context,
            );
        } catch (error) {
            if (error instanceof Error && "why" in error) {
                if (error.why === "entries" || error.why === "bytes")
                    return recordRefusal("merge", hamlet, {
                        kind: "merge-too-large",
                    });
                if (error.why === "unsafe")
                    return recordRefusal("merge", hamlet, {
                        kind: "merge-source-unsafe",
                        text: error.message,
                    });
            }
            throw error;
        }
        const from = await listTree(home, stage, ".", context);
        const to = await listTree(home, target, ".", context);
        const present = new Map(to.files.map((f) => [f.path, f.sha256]));
        const files = new Map<string, Buffer>();
        const clashes: string[] = [];
        const put = (
            rel: string,
            bytes: Buffer,
        ): Result<void, RecordRefusal> => {
            if (
                rel.startsWith("/") ||
                rel.includes("\\") ||
                rel.includes("\0") ||
                rel.split("/").some((p) => !p || p === "." || p === "..")
            )
                return recordRefusal("merge", hamlet, { kind: "merge-path" });
            if (files.has(rel) || present.has(rel))
                return recordRefusal("merge", hamlet, {
                    kind: "merge-path-taken",
                    path: rel,
                });
            files.set(rel, bytes);
            return ok(undefined);
        };
        const keptNotes = await notes(
            home,
            { hamlet, number: kept.number },
            context,
        );
        if (!keptNotes.ok) return keptNotes;
        const before = keptNotes.value.text;
        const theirBytes = from.files.some((f) => f.path === "NOTES.md")
            ? await readFile(join(stage, "NOTES.md"))
            : Buffer.from("# Notes\n");
        const theirs = theirBytes.toString("utf8");
        const both = !!substance(before) && !!substance(theirs);
        const body = substance(theirs)
            ? `${before.trimEnd()}\n\n## From ${gone.name}\n${substance(theirs)}`
            : before.trimEnd();
        const after = `${body.trimEnd()}\n\n${mergedParagraph(gone)}`;
        const beforeSaved = put(
            `merged/${gone.name}/NOTES.before.md`,
            keptNotes.value.bytes,
        );
        if (!beforeSaved.ok) return beforeSaved;
        const theirsSaved = put(`merged/${gone.name}/NOTES.md`, theirBytes);
        if (!theirsSaved.ok) return theirsSaved;
        const renamed = new Map<string, string>();
        for (const f of from.files) {
            const skill = /^skills\/([^/]+)\//.exec(f.path)?.[1];
            if (!skill || renamed.has(skill)) continue;
            const prefix = `skills/${skill}/`;
            const theirs = from.files.filter((f) => f.path.startsWith(prefix));
            const mine = to.files.filter((f) => f.path.startsWith(prefix));
            const same =
                theirs.length === mine.length &&
                theirs.every((f) => present.get(f.path) === f.sha256);
            if (same) renamed.set(skill, "");
            else if (mine.length > 0) {
                const name = `${skill}-from-${gone.name}`;
                renamed.set(skill, name);
                clashes.push(
                    `skill \`${skill}\` (the absorbed one is \`${name}\`)`,
                );
            } else renamed.set(skill, skill);
        }
        for (const f of from.files) {
            if (f.path === "NOTES.md") continue;
            let rel = f.path;
            let bytes: Buffer = await readFile(join(stage, f.path));
            const skill = /^skills\/([^/]+)\//.exec(rel)?.[1];
            if (skill) {
                const name = renamed.get(skill);
                if (!name) continue;
                if (name !== skill) {
                    rel = rel.replace(`skills/${skill}/`, `skills/${name}/`);
                    bytes = renamedSkill(bytes, skill, name);
                }
            } else if (present.has(rel)) {
                if (present.get(rel) === f.sha256) continue;
                rel = `merged/${gone.name}/${rel}`;
                clashes.push(
                    `file \`${f.path}\` (the absorbed one is \`${rel}\`)`,
                );
            }
            const placed = put(rel, bytes);
            if (!placed.ok) return placed;
        }
        for (const rel of files.keys()) {
            const parts = rel.split("/");
            let path = target;
            for (const [index, part] of parts.entries()) {
                path = join(path, part);
                const stat = await lstat(path).catch((error: unknown) => {
                    if (
                        error instanceof Error &&
                        "code" in error &&
                        error.code === "ENOENT"
                    )
                        return undefined;
                    throw error;
                });
                if (stat && (index === parts.length - 1 || !stat.isDirectory()))
                    return {
                        ok: false,
                        error: {
                            kind: "destination-blocked",
                            operation: "merge",
                            path: rel,
                            retrySafe: true,
                        },
                    };
            }
        }
        return {
            ok: true,
            value: {
                files,
                clashes,
                before,
                after,
                notes: both ? "joined" : "plain",
            },
        };
    } finally {
        await rm(stage, { recursive: true, force: true });
    }
}

function renamedSkill(bytes: Buffer, old: string, name: string): Buffer {
    if (!isUtf8(bytes)) return bytes;
    let named = false;
    return Buffer.from(
        bytes
            .toString("utf8")
            .replaceAll(`skills/${old}/`, `skills/${name}/`)
            .split("\n")
            .map((line) => {
                const colon = line.indexOf(":");
                if (
                    !named &&
                    line.slice(0, colon).trim() === "name" &&
                    line
                        .slice(colon + 1)
                        .trim()
                        .replace(/^"|"$/g, "") === old
                ) {
                    named = true;
                    return `name: ${name}`;
                }
                return line;
            })
            .join("\n"),
    );
}

async function broughtBytes(
    source: string,
    entry: Archived,
    f: { path: string; sha256: string },
): Promise<Buffer | undefined> {
    const aside = `merged/${entry.agent.name}/`;
    if (f.path === `${aside}NOTES.before.md`)
        return Buffer.from(entry.merge?.notesBefore ?? "");
    const rel = f.path.startsWith(aside) ? f.path.slice(aside.length) : f.path;
    const candidates = [f.path, rel];
    for (const path of candidates) {
        const bytes = await readFile(join(source, path)).catch(() => undefined);
        if (bytes && digest(bytes) === f.sha256) return bytes;
    }
    const name = /^skills\/([^/]+)\//.exec(rel)?.[1];
    const suffix = `-from-${entry.agent.name}`;
    if (name?.endsWith(suffix)) {
        const old = name.slice(0, -suffix.length);
        const path = rel.replace(`skills/${name}/`, `skills/${old}/`);
        const bytes = await readFile(join(source, path)).catch(() => undefined);
        if (bytes) {
            const changed = renamedSkill(bytes, old, name);
            if (digest(changed) === f.sha256) return changed;
        }
    }
    if (rel === "NOTES.md" && digest("# Notes\n") === f.sha256)
        return Buffer.from("# Notes\n");
    return undefined;
}

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

async function apply(
    home: Home,
    hamlet: string,
    entry: Archived,
    context: Context,
): Promise<FileResult> {
    const merge = entry.merge;
    if (!merge || merge.applied) return ok(undefined);
    await stopAgent(home, { hamlet, number: entry.agent.number }, context);
    await stopAgent(home, { hamlet, number: merge.kept.number }, context);
    const source = await mkdtemp(join(tmpdir(), "hamlet-merge-source-"));
    const delivery = await mkdtemp(join(tmpdir(), "hamlet-merge-delivery-"));
    const target = houseOf(home.layout, { hamlet, number: merge.kept.number });
    try {
        await stageTree(
            home,
            {
                root: houseOf(home.layout, {
                    hamlet,
                    number: entry.agent.number,
                }),
                rel: ".",
            },
            source,
            MERGE_CAPS,
            context,
        );
        const present = new Map(
            (await listTree(home, target, ".", context)).files.map((f) => [
                f.path,
                f.sha256,
            ]),
        );
        const pending: string[] = [];
        for (const f of merge.brought) {
            if (present.get(f.path) === f.sha256) continue;
            if (present.has(f.path)) return pendingFiles(hamlet, entry);
            const bytes = await broughtBytes(source, entry, f);
            if (!bytes) return pendingFiles(hamlet, entry);
            const staged = join(delivery, "tree", f.path);
            await mkdir(dirname(staged), { recursive: true });
            await writeFile(staged, bytes);
            pending.push(f.path);
        }
        if (pending.length) {
            await writeFile(
                join(delivery, "manifest.json"),
                JSON.stringify(pending),
            );
            const copier = treeWorker(home, {
                root: delivery,
                stage: target,
                caps: MERGE_CAPS,
            });
            try {
                const copied = await copier.exec(
                    [
                        process.execPath,
                        "-e",
                        String.raw`
const fs = require("node:fs"), p = require("node:path");
const [root, stage] = process.argv.slice(1);
for (const rel of JSON.parse(fs.readFileSync(p.join(stage, "manifest.json"), "utf8"))) {
    const parts = rel.split("/");
    if (!rel || rel.includes("\\") || parts.some(s => !s || s === "." || s === "..")) throw Error("relative path required");
    let parent = root;
    for (const part of parts.slice(0, -1)) {
        parent = p.join(parent, part);
        try { fs.mkdirSync(parent); } catch (e) { if (e.code !== "EEXIST") throw e; }
        if (!fs.lstatSync(parent).isDirectory()) throw Error("real parent required");
    }
    fs.copyFileSync(p.join(stage, "tree", rel), p.join(root, rel), fs.constants.COPYFILE_EXCL);
}`,
                        target,
                        delivery,
                    ],
                    undefined,
                    context,
                );
                if (!copied.ok || copied.value.exitCode !== 0)
                    return {
                        ok: false,
                        error: {
                            kind: "pending-files",
                            operation: "merge",
                            hamlet,
                            archive: entry.id,
                            kept: merge.kept.name,
                            absorbed: entry.agent.name,
                            retrySafe: true,
                        },
                    };
            } finally {
                await copier.cleanup(context);
            }
        }
        const env = envFor(home, { hamlet, number: merge.kept.number }, true);
        try {
            const read = await notes(
                home,
                { hamlet, number: merge.kept.number },
                context,
            );
            if (!read.ok) return read;
            const current = read.value.text;
            if (current !== merge.notesBefore && current !== merge.notesAfter)
                return {
                    ok: false,
                    error: {
                        kind: "changed-notes",
                        operation: "merge",
                        hamlet,
                        archive: entry.id,
                        kept: merge.kept.name,
                        absorbed: entry.agent.name,
                        retrySafe: true,
                    },
                };
            const result = await env.writeFile(
                join(target, "NOTES.md"),
                merge.notesAfter,
                context,
            );
            if (!result.ok) return pendingFiles(hamlet, entry);
        } finally {
            await env.cleanup(context);
        }
        await home.harness.commit(async (tx) => {
            const a = (await tx.doc(RecordDoc, hamlet, hamlet)).archive.find(
                (a) => a.id === entry.id,
            );
            if (a?.merge) a.merge.applied = true;
        }, context);
    } finally {
        await rm(source, { recursive: true, force: true });
        await rm(delivery, { recursive: true, force: true });
    }
    return ok(undefined);
}

function mergedParagraph(gone: AgentRecord): string {
    return `## Merged\n- \`${gone.name}\` was merged into me. Its purpose was: ${gone.purpose} Its notes as they were are in merged/${gone.name}/NOTES.md and mine in merged/${gone.name}/NOTES.before.md. Files of its that differed from mine of the same name are under merged/${gone.name}/.\n`;
}

function mergeRequest(
    kept: AgentRecord,
    gone: AgentRecord,
    purpose: string,
    limit: number,
): string {
    return `You are \`${kept.name}\`, an agent in a hamlet of agents. The mayor is merging the agent \`${gone.name}\` into you: from now on you do the work of both, under your own name. Your purpose was: ${kept.purpose} Its purpose was: ${gone.purpose} Your purpose becomes: ${purpose.trim()}\nYou are writing your new NOTES.md, the memory that is shown to you on every turn.\nWrite the one NOTES.md you will keep.\n- Keep every fact, preference and lesson from both. Drop nothing because it looks minor.\n- Where both say the same thing, say it once.\n- Where they disagree, keep both and say which of you held which.\n- Speak as yourself: things \`${gone.name}\` wrote as "I" are now things you know or do.\n- Group it by subject, not by who wrote it.\n- At most ${limit} characters.\n- Start with the line \`# Notes\`. Reply with the file's text only: no code fence, no comment before or after.`;
}

function accept(reply: string, limit: number): string | undefined {
    let text = reply.trim();
    if (text.includes("</think>"))
        text = text.slice(text.lastIndexOf("</think>") + 8).trim();
    if (text.startsWith("```"))
        text = text
            .slice(text.indexOf("\n") + 1)
            .replace(/```\s*$/, "")
            .trim();
    if (text.includes("# Notes")) text = text.slice(text.indexOf("# Notes"));
    return substance(text) && Array.from(text).length <= limit
        ? text
        : undefined;
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
    );
    return ok(outcome.ok ? pending.value.words : pendingText(outcome.error));
}

async function part(
    home: Home,
    hamlet: string,
    entry: Archived,
    context: Context,
): Promise<FileResult> {
    const merge = entry.merge;
    if (!merge || merge.applied) return ok(undefined);
    const ref = { hamlet, number: merge.kept.number };
    await stopAgent(home, ref, context);
    const root = houseOf(home.layout, ref);
    const env = envFor(home, ref, true);
    try {
        const read = await notes(home, ref, context, "split");
        if (!read.ok) return read;
        const current = read.value.text;
        const clean =
            current === merge.notesAfter || current === merge.notesBefore;
        const said = `\`${entry.agent.name}\` was split off again`;
        const text = clean
            ? merge.notesBefore
            : current.includes(said)
              ? current
              : `${current.trimEnd()}\n\n## Split\n- ${said} and has its own house. What these notes hold of its work is its business now. My notes from before the merge are in merged/${entry.agent.name}/NOTES.before.md.\n`;
        const written = await env.writeFile(
            join(root, "NOTES.md"),
            text,
            context,
        );
        if (!written.ok) return pendingFiles(hamlet, entry);
        const listing = new Map(
            (await listTree(home, root, ".", context)).files.map((f) => [
                f.path,
                f.sha256,
            ]),
        );
        const paths = merge.brought
            .filter(
                (f) =>
                    listing.get(f.path) === f.sha256 &&
                    (clean || dirname(f.path) !== `merged/${entry.agent.name}`),
            )
            .map((f) => f.path);
        if (paths.length) {
            await removeTree(home, root, paths, context);
            const trimmed = await env.exec(
                [
                    process.execPath,
                    "-e",
                    `const fs=require("node:fs"),p=require("node:path"); const [root,...files]=process.argv.slice(1); for(const file of files) { let dir=p.dirname(p.join(root,file)); while(dir!==root && dir.startsWith(root+p.sep)) { try { fs.rmdirSync(dir); } catch { break; } dir=p.dirname(dir); } }`,
                    root,
                    ...paths,
                ],
                undefined,
                context,
            );
            if (!trimmed.ok || trimmed.value.exitCode !== 0)
                return pendingFiles(hamlet, entry);
        }
        await home.harness.commit(async (tx) => {
            const a = (await tx.doc(RecordDoc, hamlet, hamlet)).archive.find(
                (a) => a.id === entry.id,
            );
            if (a?.merge) a.merge.applied = true;
        }, context);
    } finally {
        await env.cleanup(context);
    }
    return ok(undefined);
}

type PendingFiles = {
    kind: "pending-files" | "changed-notes";
    operation: "merge" | "split";
    hamlet: string;
    archive: string;
    kept: string;
    absorbed: string;
    retrySafe: true;
};

type FileResult = Result<void, PendingFiles | NotesFailure>;

function pendingFiles(
    hamlet: string,
    entry: Archived,
): { ok: false; error: PendingFiles } {
    const merge = entry.merge;
    if (!merge) throw new Error("File work requires a checked merge manifest.");
    return {
        ok: false,
        error: {
            kind: "pending-files",
            operation: entry.back === null ? "merge" : "split",
            hamlet,
            archive: entry.id,
            kept: merge.kept.name,
            absorbed: entry.agent.name,
            retrySafe: true,
        },
    };
}

function pendingText(error: PendingFiles | NotesFailure): string {
    switch (error.kind) {
        case "notes-unreadable":
            return notesFailureText(error);
        case "changed-notes":
            return `The merge of \`${error.archive}\` is pending because the kept agent's notes changed. Both versions are kept. Save its newer NOTES.md aside and put its merged/${error.absorbed}/NOTES.before.md back as NOTES.md, then use /merge ${error.archive} into ${error.kept} to retry safely. Do not erase its house.`;
        case "pending-files":
            return `The ${error.operation} of \`${error.archive}\` in \`${error.hamlet}\` is recorded but its files are still pending. Inspect both houses and keep conflicting paths aside, then use ${error.operation === "merge" ? `/merge ${error.archive} into ${error.kept}` : `/restore ${error.archive}`} to retry safely. Do not erase its house.`;
    }
    const unhandled: never = error;
    throw new Error(`Unclassified pending files: ${unhandled}`);
}

async function retryFiles(
    home: Home,
    hamlet: string,
    entry: Archived,
    context: Context,
): Promise<FileResult> {
    const merge = entry.merge;
    if (!merge) throw new Error("File work needs a merge archive.");
    let failure: PendingFiles | NotesFailure | undefined;
    try {
        failure = await stoppedHouses(
            home,
            hamlet,
            [entry.agent, merge.kept],
            context,
            async () => {
                if (entry.back === null) {
                    const outcome = await apply(home, hamlet, entry, context);
                    return outcome.ok ? undefined : outcome.error;
                }
                const outcome = await part(home, hamlet, entry, context);
                return outcome.ok ? undefined : outcome.error;
            },
        );
    } catch {
        failure = pendingFiles(hamlet, entry).error;
    }
    if (failure) {
        const text = safeWords(home, pendingText(failure));
        console.error(text);
        await home.harness.commit(
            (tx) => tellChiefs(home, tx, hamlet, text),
            context,
        );
        return { ok: false, error: failure };
    }
    return ok(undefined);
}

function notesFailureText(error: NotesFailure): string {
    switch (error.kind) {
        case "notes-unreadable":
            return `The ${error.operation} cannot use the kept agent's NOTES.md (${agentKey(error.agent)}): ${error.reason === "not-utf8" ? "it is not UTF-8 text" : "it could not be read safely"}. NOTES.md was not replaced. Keep its original bytes aside and correct the file, then read /record before retrying /merge or /restore <archive> for pending work.`;
    }
    const unhandled: never = error.kind;
    throw new Error(`Unclassified notes failure: ${unhandled}`);
}
