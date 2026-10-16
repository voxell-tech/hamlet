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
import { AgentConversationsDoc, WithDoc } from "../chats/conversations.ts";
import { InstructionsDoc, owed } from "../chats/instructions.ts";
import { stopAgent } from "../chats.ts";
import { revoke } from "../gate.ts";
import { untilAborted } from "../home/abort.ts";
import { localDay } from "../home/clock.ts";
import { houseOf } from "../home/layout.ts";
import type { Deed, Home, Part } from "../home.ts";
import { removeHouse } from "../house.ts";
import { LimitsDoc } from "../limits/rules.ts";
import { resetTrust } from "../oversight/asked.ts";
import { lapseAgent } from "../oversight/waits.ts";
import { isChief, liveHamlets, RealmDoc } from "../realm/places.ts";
import { farewell, roadsOf } from "../realm/roads.ts";
import {
    type AgentRef,
    type Archived,
    agentKey,
    checkName,
    type Decision,
    decisionText,
    type HamletRecord,
    mayDecide,
    named,
    RecordDoc,
} from "../record.ts";
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
                    const result = await api.commit(async (tx) => {
                        const saved = await tx.doc(RemovalDoc, api.taskId);
                        if (saved.text) return { ...saved };
                        const place = await tx.doc(WithDoc, api.conversationId);
                        const r = await tx.doc(
                            RecordDoc,
                            place.hamlet,
                            place.hamlet,
                        );
                        if (
                            !r.agents.some((a) => a.number === place.number) ||
                            r.mayor !== place.number
                        )
                            return {
                                number: null,
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
                                text: decisionText(decision.error),
                            };
                        const gone = await depart(
                            home,
                            tx,
                            place.hamlet,
                            args.name,
                            args.reason,
                            "mayor",
                        );
                        Object.assign(saved, {
                            number: gone.agent.number,
                            text: removedWords(gone),
                        });
                        return { ...saved };
                    }, context);
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
                    return { output: [{ type: "text", text: result.text }] };
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
export const remove: Deed = async (home, given, context) => {
    const [name = "", ...reason] = given.rest.trim().split(/\s+/);
    const gone = await home.harness.commit(async (tx) => {
        await chief(tx, given.hamlet, given.chief);
        return depart(
            home,
            tx,
            given.hamlet,
            name,
            reason.join(" "),
            `chief:${given.chief}`,
        );
    }, context);
    await stopAgent(
        home,
        { hamlet: given.hamlet, number: gone.agent.number },
        context,
    );
    return removedWords(gone);
};

async function chief(tx: Tx, hamlet: string, name: string): Promise<void> {
    if (!isChief(await tx.doc(RealmDoc), hamlet, name))
        throw new Error("Only a chief of this hamlet can do this.");
}

async function depart(
    home: Home,
    tx: Tx,
    hamlet: string,
    name: string,
    reason: string,
    by: string,
): Promise<Archived> {
    const r = await tx.doc(RecordDoc, hamlet, hamlet);
    const agent = named(r, name);
    if (agent?.number === r.mayor || name === "mayor")
        throw new Error(
            "the mayor cannot be removed or merged; replacing a mayor takes a chief or a vote",
        );
    if (!agent) {
        const gone = r.archive.findLast(
            (a) => a.agent.name === name && a.back === null,
        );
        throw new Error(
            gone ? farewell(gone, r) : `no agent named \`${name}\``,
        );
    }
    if (!reason.trim())
        throw new Error("give a `reason`; it is kept in the record");
    if (by === "mayor") {
        const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
        const i = owed(
            await tx.doc(
                InstructionsDoc,
                agentKey({ hamlet, number: agent.number }),
                agentKey({ hamlet, number: agent.number }),
            ),
            home.now(),
            limits.leaving.protectedMinutes * 60_000,
        );
        if (i)
            throw new Error(
                `\`${name}\` has a task from the chief ${i.chief} ("${Array.from(i.text).slice(0, 80).join("")}") that ${i.done === null ? "is not finished" : "has only just ended"}. No agent may be removed or merged to avoid a chief's task. Wait until the chief has had the answer, or ask the chief to do this.`,
            );
    }
    const at = home.now();
    let stamp = at;
    while (r.archive.some((a) => a.id === `${name}-${stamp}`)) stamp++;
    const entry: Archived = {
        id: `${name}-${stamp}`,
        agent: { ...agent },
        how: "removed",
        into: null,
        at,
        by,
        reason: reason.trim(),
        roads: roadsOf(r).filter(
            (r) => r.a === agent.number || r.b === agent.number,
        ),
        merge: null,
        back: null,
    };
    await lapseAgent(tx, { hamlet, number: agent.number }, at);
    await lapseTrust(tx, { hamlet, number: agent.number });
    for (const permission of ["network", "unconfined"] as const)
        await revoke(tx, { hamlet, number: agent.number }, permission, by, at);
    r.agents = r.agents.filter((a) => a.number !== agent.number);
    r.roads = r.roads.filter(
        (r) => r.a !== agent.number && r.b !== agent.number,
    );
    r.archive.push(entry);
    r.decisions.push({
        kind: "removed",
        at,
        by,
        reason: entry.reason,
        agent: agent.number,
        name,
        archive: entry.id,
    });
    return JSON.parse(JSON.stringify(entry)) as Archived;
}

function removedWords(entry: Archived): string {
    return `Removed \`${entry.agent.name}\`. Its house is archived as ${entry.id} and its roads are gone. A chief can restore it.`;
}

/** A chief's restoration deed. */
export const restore: Deed = async (home, given, context) => {
    const [which = "", ...rest] = given.rest.trim().split(/\s+/);
    const rename = rest[0] === "as";
    const entry = await home.harness.commit(async (tx) => {
        await chief(tx, given.hamlet, given.chief);
        const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
        const entry = r.archive.findLast(
            (a) =>
                (a.back === null || (a.merge && !a.merge.applied)) &&
                (a.id === which || a.agent.name === which),
        );
        if (!entry)
            throw new Error(
                `nothing is archived under \`${which}\`; /record lists what is`,
            );
        return JSON.parse(JSON.stringify(entry)) as Archived;
    }, context);
    const name = rename ? (rest[1] ?? "") : entry.agent.name;
    const reason = rest.slice(rename ? 2 : 0).join(" ") || "a chief asked";
    if (entry.how === "merged")
        return split(
            home,
            { hamlet: given.hamlet, entry, name, reason, chief: given.chief },
            context,
        );
    return home.harness.commit(async (tx) => {
        await chief(tx, given.hamlet, given.chief);
        const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
        const archived = r.archive.find(
            (a) => a.id === entry.id && a.back === null,
        );
        if (!archived)
            throw new Error(
                `nothing is archived under \`${which}\`; /record lists what is`,
            );
        if (name !== archived.agent.name) checkName(name);
        for (const h of liveHamlets(await tx.doc(RealmDoc)))
            if (
                (await tx.doc(RecordDoc, h.id, h.id)).agents.some(
                    (a) => a.name === name,
                )
            )
                throw new Error(
                    `an agent named \`${name}\` lives in the hamlet now. Restore this one under another name: /restore ${which} as <new-name>`,
                );
        const limits = await tx.doc(LimitsDoc, given.hamlet, given.hamlet);
        if (r.agents.length - 1 >= limits.mostAgents)
            throw new Error(
                `the hamlet is full (${limits.mostAgents} agents); remove one first, or raise mostAgents in the limits`,
            );
        archived.back = name;
        r.agents.push({ ...archived.agent, name });
        for (const road of archived.roads)
            if (
                [road.a, road.b].every(
                    (end) =>
                        end === "office" ||
                        r.agents.some((a) => a.number === end),
                ) &&
                !r.roads.some(
                    (r) =>
                        (r.a === road.a && r.b === road.b) ||
                        (r.a === road.b && r.b === road.a),
                )
            )
                r.roads.push({ ...road });
        r.decisions.push({
            kind: "restored",
            at: home.now(),
            by: `chief:${given.chief}`,
            reason,
            agent: entry.agent.number,
            name,
            archive: entry.id,
        });
        const peers = roadsOf(r)
            .filter(
                (road) =>
                    road.a === entry.agent.number ||
                    road.b === entry.agent.number,
            )
            .map((road) => {
                const end = road.a === entry.agent.number ? road.b : road.a;
                return end === "office"
                    ? "mayor"
                    : (r.agents.find((a) => a.number === end)?.name ?? "");
            });
        const built = roadsOf(r).filter(
            (r) => r.a !== "office" && r.b !== "office",
        ).length;
        return `Restored \`${name}\` with its house as it was. Its roads lead to: ${peers.join(", ")}.${built > limits.mostRoads ? ` The hamlet now has ${built} roads between agents, more than the ${limits.mostRoads} your limits allow. They all work; the mayor can build no more until you raise mostRoads in the limits.` : ""}`;
    }, context);
};

type EraseFailure = {
    kind: "merge-pending";
    operation: "erase";
    hamlet: string;
    archive: string;
    retrySafe: true;
};

/** A chief's permanent erasure deed. */
export const erase: Deed = async (home, given, context) =>
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
                | { ok: false; error: EraseFailure }
            > => {
                await chief(tx, given.hamlet, given.chief);
                const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
                const entry = r.archive.findLast(
                    (a) =>
                        a.back === null &&
                        (a.id === which || a.agent.name === which),
                );
                if (!entry)
                    throw new Error(
                        `nothing is archived under \`${which}\`. Only an agent that was removed or merged can be deleted for good; /remove it first`,
                    );
                if (entry.merge && !entry.merge.applied)
                    return {
                        ok: false,
                        error: {
                            kind: "merge-pending",
                            operation: "erase",
                            hamlet: given.hamlet,
                            archive: entry.id,
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
            switch (error.kind) {
                case "merge-pending":
                    return `Cannot erase \`${error.archive}\`: its merge has not finished. Inspect its houses and use /restore ${error.archive} to finish or split it first.`;
            }
            const unhandled: never = error.kind;
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
        return `Deleted \`${entry.name}\` for good. Freed ${bytes} bytes. It cannot be restored.`;
    });

/** The hamlet's history and archived agents. */
export const record: Deed = async (home, given, context) => {
    await home.harness.commit(
        (tx) => chief(tx, given.hamlet, given.chief),
        context,
    );
    return (
        (await recordLines(home, given.hamlet, context)).join("\n") ||
        "The record is empty."
    );
};

/** The latest archived agent's restoration. */
export const undo: Deed = async (home, given, context) => {
    const r = await home.harness.snapshot(RecordDoc, given.hamlet, context);
    const latest = r?.archive.findLast((a) => a.back === null);
    return latest
        ? restore(home, { ...given, rest: latest.id }, context)
        : "Nothing to undo. Only a removal or a merge can be undone (the archive keeps the house); a permission is taken back with /revoke. A command that ran, a file that was changed and what was spent cannot be undone.";
};

/** The hamlet's history in words. */
export async function recordLines(
    home: Home,
    hamlet: string,
    context: Context,
): Promise<string[]> {
    const r = await home.harness.snapshot(RecordDoc, hamlet, context);
    if (!r) return [];
    const name = (n: number | "office") =>
        n === "office"
            ? "mayor"
            : (r.agents.find((a) => a.number === n)?.name ??
              r.archive.findLast((a) => a.agent.number === n)?.agent.name ??
              r.decisions
                  .filter((d) => d.kind === "created")
                  .find((d) => d.agent === n)?.name ??
              String(n));
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
    const by =
        "by" in d
            ? d.by === "mayor"
                ? "the mayor"
                : d.by.startsWith("chief:")
                  ? `the chief ${d.by.slice(6)}`
                  : d.by
            : "";
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

/** Takes edit and command trust back from every conversation of a departing agent. */
export async function lapseTrust(tx: Tx, agent: AgentRef): Promise<void> {
    const key = agentKey(agent);
    for (const id of (await tx.doc(AgentConversationsDoc, key, key)).ids)
        await resetTrust(tx, await tx.doc(WithDoc, id));
}
