import type { Context } from "@earendil-works/chord";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { Type } from "@earendil-works/pi-ai";
import {
    defineDoc,
    defineExtension,
    defineTool,
    GenerationTask,
    hook,
    ToolTask,
    type Tx,
} from "@earendil-works/pi-durable";
import { WithDoc } from "../chats/conversations.ts";
import { stopAgent } from "../chats.ts";
import { untilAborted } from "../home/abort.ts";
import { localDay } from "../home/clock.ts";
import { houseOf } from "../home/layout.ts";
import type { Home, Part } from "../home.ts";
import { removeHouse } from "../house.ts";
import { lapseAgent } from "../oversight/waits.ts";
import { isChief, liveHamlets, RealmDoc } from "../realm/places.ts";
import { farewell } from "../realm/roads.ts";
import {
    type Archived,
    byWords,
    copyArchive,
    type Decision,
    depart,
    endName,
    type HamletRecord,
    lapseTrust,
    mayDecide,
    memoised,
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
import { serialHouseWork, split } from "./merging.ts";

const RemovalDoc = defineDoc<{ number: number | null; text: string }>({
    kind: "hamlet.removal-call",
    version: 1,
    scope: "task",
    initial: () => ({ number: null, text: "" }),
});

/** Agents leaving and coming back. */
export const Leaving: Part = (home) => ({
    extension: defineExtension({
        name: "hamlet.leaving",
        hooks: [
            hook(GenerationTask, {
                beforeRequest: async (_request, api, context) => {
                    const place = await api.snapshot(
                        WithDoc,
                        api.conversationId,
                        context,
                    );
                    if (!place) return undefined;
                    const r = await api.snapshot(
                        RecordDoc,
                        place.hamlet,
                        context,
                    );
                    if (r?.agents.some((a) => a.number === place.number))
                        return undefined;
                    void home.harness.abortTask(api.taskId, BACKGROUND_CONTEXT);
                    return untilAborted(context);
                },
            }),
            hook(ToolTask, {
                beforeTool: async (_call, api, context) => {
                    const place = await api.snapshot(
                        WithDoc,
                        api.conversationId,
                        context,
                    );
                    if (!place) return undefined;
                    const r = await api.snapshot(
                        RecordDoc,
                        place.hamlet,
                        context,
                    );
                    if (r?.agents.some((a) => a.number === place.number))
                        return undefined;
                    return {
                        block: "This agent has left the hamlet. Ask a chief to restore it before giving it more work.",
                    };
                },
            }),
        ],
        tools: [
            defineTool({
                name: "remove_agent",
                description:
                    "Remove an agent, archiving its house and roads. A reason is kept in the record.",
                parameters: Type.Object({
                    name: Type.String(),
                    reason: Type.String(),
                }),
                replay: "safe",
                execute: async (args, api, context) => {
                    const result = await memoised(
                        api,
                        {
                            read: async (tx) => {
                                const saved = await tx.doc(
                                    RemovalDoc,
                                    api.taskId,
                                );
                                return saved.text
                                    ? {
                                          number: saved.number,
                                          text: saved.text,
                                          isError: false,
                                      }
                                    : undefined;
                            },
                            change: async (tx) => {
                                const place = await tx.doc(
                                    WithDoc,
                                    api.conversationId,
                                );
                                const mayor = await requireMayor(tx, place);
                                if (!mayor.ok)
                                    return {
                                        number: null,
                                        isError: false,
                                        text: "Only the mayor can remove an agent.",
                                    };
                                const decision = await mayDecide(
                                    tx,
                                    place.hamlet,
                                    home.now(),
                                    "remove",
                                );
                                if (!decision.ok)
                                    return {
                                        number: null,
                                        isError: false,
                                        text: toolText(decision.error),
                                    };
                                const gone = await depart(
                                    tx,
                                    place.hamlet,
                                    args.name,
                                    {
                                        by: "mayor",
                                        reason: args.reason,
                                        how: "removed",
                                        at: home.now(),
                                    },
                                );
                                return gone.ok
                                    ? {
                                          number: gone.value.agent.number,
                                          isError: false,
                                          text: removedWords(gone.value),
                                      }
                                    : {
                                          number: null,
                                          isError: true,
                                          text: toolText(gone.error),
                                      };
                            },
                            write: async (tx, result) => {
                                if (result.number !== null)
                                    Object.assign(
                                        await tx.doc(RemovalDoc, api.taskId),
                                        {
                                            number: result.number,
                                            text: result.text,
                                        },
                                    );
                            },
                        },
                        context,
                    );
                    const place = await api.snapshot(
                        WithDoc,
                        api.conversationId,
                        context,
                    );
                    if (result.number !== null && place)
                        await stopAgent(
                            home,
                            { hamlet: place.hamlet, number: result.number },
                            context,
                        );
                    return result.isError
                        ? refusedTool(result.text)
                        : { output: [{ type: "text", text: result.text }] };
                },
            }),
        ],
    }),
    async start(context) {
        const realm = await home.harness.snapshot(RealmDoc, context);
        for (const { id } of liveHamlets(realm)) {
            const r = await home.harness.snapshot(RecordDoc, id, context);
            for (const entry of r?.archive ?? []) {
                if (entry.back !== null) continue;
                const ref = { hamlet: id, number: entry.agent.number };
                try {
                    await home.harness.commit(async (tx) => {
                        await lapseAgent(tx, ref, home.now());
                        await lapseTrust(tx, ref);
                    }, context);
                    await stopAgent(home, ref, context);
                } catch {
                    console.error(
                        `The departed agent \`${entry.id}\` could not be stopped. Its model and tools remain blocked; inspect its archived house before restoring it.`,
                    );
                }
            }
        }
    },
});

/** A chief's removal deed. */
export const remove: RecordDeed = async (home, given, context) => {
    const [name = "", ...reason] = given.rest.trim().split(/\s+/);
    const gone = await home.harness.commit(async (tx) => {
        const access = await chief(tx, given.hamlet, given.chief);
        if (!access.ok) return access;
        return depart(tx, given.hamlet, name, {
            by: `chief:${given.chief}`,
            reason: reason.join(" "),
            how: "removed",
            at: home.now(),
        });
    }, context);
    if (!gone.ok) return gone;
    await stopAgent(
        home,
        { hamlet: given.hamlet, number: gone.value.agent.number },
        context,
    );
    return ok(removedWords(gone.value));
};
async function chief(
    tx: Tx,
    hamlet: string,
    name: string,
): Promise<Result<void, RecordRefusal>> {
    return isChief(await tx.doc(RealmDoc), hamlet, name)
        ? ok(undefined)
        : recordRefusal("chief", hamlet, { kind: "record-chief" });
}

function removedWords(entry: Archived): string {
    return `Removed \`${entry.agent.name}\`. Its house is archived as ${entry.id} and its roads are gone. A chief can restore it.`;
}

/** A chief's restoration deed. */
export const restore: RecordDeed = async (home, given, context) => {
    const [which = "", ...rest] = given.rest.trim().split(/\s+/);
    const rename = rest[0] === "as";
    const archived = await home.harness.commit(async (tx) => {
        const access = await chief(tx, given.hamlet, given.chief);
        if (!access.ok) return access;
        const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
        const entry = r.archive.findLast(
            (a) =>
                (a.back === null || (a.merge && !a.merge.applied)) &&
                (a.id === which || a.agent.name === which),
        );
        return entry
            ? ok(copyArchive(entry))
            : recordRefusal("restore", given.hamlet, {
                  kind: "archive-missing",
                  archive: which,
              });
    }, context);
    if (!archived.ok) return archived;
    const entry = archived.value;
    const name = rename ? (rest[1] ?? "") : entry.agent.name;
    const reason = rest.slice(rename ? 2 : 0).join(" ") || "a chief asked";
    if (entry.how === "merged")
        return split(
            home,
            {
                hamlet: given.hamlet,
                entry,
                name,
                reason,
                chief: given.chief,
            },
            context,
        );
    return home.harness.commit(async (tx) => {
        const access = await chief(tx, given.hamlet, given.chief);
        if (!access.ok) return access;
        const restored = await restoreRecord(tx, given.hamlet, entry.id, {
            name,
            by: `chief:${given.chief}`,
            reason,
            at: home.now(),
        });
        if (!restored.ok) return restored;
        return ok(restored.value.text);
    }, context);
};

type EraseFailure = {
    kind: "merge-pending";
    operation: "erase";
    hamlet: string;
    archive: string;
    kept: string;
    retrySafe: true;
};

/** A chief's permanent erasure deed. */
export const erase: RecordDeed = async (home, given, context) =>
    serialHouseWork(home, context, async () => {
        const which = given.rest.trim();
        const outcome = await home.harness.commit(
            async (
                tx,
            ): Promise<
                | {
                      ok: true;
                      value: Pick<Archived["agent"], "name" | "number">;
                  }
                | { ok: false; error: EraseFailure | RecordRefusal }
            > => {
                const access = await chief(tx, given.hamlet, given.chief);
                if (!access.ok) return access;
                const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
                const entry = r.archive.findLast(
                    (a) =>
                        a.back === null &&
                        (a.id === which || a.agent.name === which),
                );
                if (!entry)
                    return recordRefusal("erase", given.hamlet, {
                        kind: "erase-missing",
                        archive: which,
                    });
                if (entry.merge && !entry.merge.applied)
                    return {
                        ok: false,
                        error: {
                            kind: "merge-pending",
                            operation: "erase",
                            hamlet: given.hamlet,
                            archive: entry.id,
                            kept:
                                r.agents.find((a) => a.number === entry.into)
                                    ?.name ?? entry.merge.kept.name,
                            retrySafe: true,
                        },
                    };
                r.archive = r.archive.filter((a) => a !== entry);
                r.decisions.push({
                    kind: "erased",
                    at: home.now(),
                    by: `chief:${given.chief}`,
                    reason: "",
                    agent: entry.agent.number,
                    name: entry.agent.name,
                    archive: entry.id,
                });
                return {
                    ok: true,
                    value: {
                        name: entry.agent.name,
                        number: entry.agent.number,
                    },
                };
            },
            context,
        );
        if (!outcome.ok) {
            const error = outcome.error;
            if (error.kind !== "merge-pending") return err(error);
            switch (error.kind) {
                case "merge-pending":
                    return ok(
                        `Cannot erase \`${error.archive}\`: its merge has not finished. Inspect its houses and use /merge ${error.archive} into ${error.kept} to finish it first.`,
                    );
            }
            const unhandled: never = error;
            throw new Error(`Unclassified erasure failure: ${unhandled}`);
        }
        const entry = outcome.value;
        const bytes = await removeHouse(
            home,
            houseOf(home.layout, {
                hamlet: given.hamlet,
                number: entry.number,
            }),
        );
        return ok(
            `Deleted \`${entry.name}\` for good. Freed ${bytes} bytes. It cannot be restored.`,
        );
    });

/** The hamlet's history and archived agents. */
export const record: RecordDeed = async (home, given, context) => {
    const access = await home.harness.commit(
        (tx) => chief(tx, given.hamlet, given.chief),
        context,
    );
    if (!access.ok) return access;
    return ok(
        (await recordLines(home, given.hamlet, context)).join("\n") ||
            "The record is empty.",
    );
};

/** The latest archived agent's restoration. */
export const undo: RecordDeed = async (home, given, context) => {
    const r = await home.harness.snapshot(RecordDoc, given.hamlet, context);
    const latest = r?.archive.findLast((a) => a.back === null);
    return latest
        ? restore(home, { ...given, rest: latest.id }, context)
        : ok(
              "Nothing to undo. Only a removal or a merge can be undone (the archive keeps the house); a permission is taken back with /revoke. A command that ran, a file that was changed and what was spent cannot be undone.",
          );
};

/** The hamlet's history in words. */
export async function recordLines(
    home: Home,
    hamlet: string,
    context: Context,
): Promise<string[]> {
    const r = await home.harness.snapshot(RecordDoc, hamlet, context);
    if (!r) return [];
    const name = (n: number | "office") => endName(r, n);
    const lines = r.decisions.flatMap((d) => {
        const text = history(d, r, name);
        return text
            ? [
                  `${localDay(d.at)} ${String(new Date(d.at).getHours()).padStart(2, "0")}:${String(new Date(d.at).getMinutes()).padStart(2, "0")}  ${text}`,
              ]
            : [];
    });
    for (const a of r.archive.filter((a) => a.back === null))
        lines.push(`${a.id}: ${farewell(a, r)} /restore ${a.id}`);
    return lines;
}

function history(
    d: Decision,
    r: Readonly<HamletRecord>,
    name: (n: number | "office") => string,
): string | undefined {
    const by = "by" in d ? byWords(d.by) : "";
    switch (d.kind) {
        case "created":
            return `the mayor created \`${d.name}\` (${d.purpose}): ${d.reason}`;
        case "road_built":
        case "road_removed":
            return `${by} ${d.kind === "road_built" ? "built a road" : "took up the road"} between \`${name(d.a)}\` and \`${name(d.b)}\`: ${d.reason}`;
        case "skill_given":
            return `${by} gave \`${name(d.to)}\` the skill \`${d.skill}\`${d.from ? ` of \`${d.from}\`` : ""}`;
        case "removed":
            return `${by} removed \`${d.name}\`: ${d.reason} [archive ${d.archive}]`;
        case "restored":
            return `${by} restored \`${d.name}\` from archive ${d.archive}: ${d.reason}`;
        case "erased":
            return `${by} deleted \`${d.name}\` for good [archive ${d.archive}]`;
        case "merged":
            return `${by} merged \`${name(d.absorb)}\` into \`${name(d.keep)}\`, which is now for: ${d.purpose} Reason: ${d.reason} [archive ${r.archive.find((a) => a.at === d.at && a.agent.number === d.absorb)?.id ?? "erased"}, notes ${d.notes}]`;
        case "mayor_changed":
            return `\`${name(d.to)}\` became mayor in place of \`${name(d.from)}\` (${d.by}): ${d.reason}`;
        case "moved_in":
        case "moved_out":
            return `${by} moved \`${d.name}\` ${d.kind === "moved_in" ? "from" : "to"} \`${d.hamlet}\`: ${d.reason}`;
        default:
            return undefined;
    }
}
