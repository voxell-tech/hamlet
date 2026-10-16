import type { Tx } from "@earendil-works/pi-durable";
import { AgentConversationsDoc, WithDoc } from "../chats/conversations.ts";
import { InstructionsDoc, owed } from "../chats/instructions.ts";
import { revoke } from "../gate/grants.ts";
import { LimitsDoc } from "../limits/rules.ts";
import { resetTrust } from "../oversight/asked.ts";
import { lapseAgent } from "../oversight/waits.ts";
import { CharacterDoc } from "../prompt/character.ts";
import { isChief, liveHamlets, RealmDoc } from "../realm/places.ts";
import { ok, type Result } from "../result.ts";
import {
    type AgentRecord,
    type AgentRef,
    type Archived,
    agentKey,
    RecordDoc,
} from "./document.ts";
import { checkName, endName, farewell, named, tidyName } from "./names.ts";
import { type RecordRefusal, refusal } from "./refusals.ts";
import { layRoad, roadsOf } from "./roads.ts";

/** A restored identity and the archive whose house must be brought back. */
export type Restored = { agent: AgentRecord; archive: Archived; text: string };

/** An admitted identity and any roads refused without undoing its admission. */
export type Admission = AgentRef & {
    roadRefusals?: { peer: string; refusal: RecordRefusal }[];
};

/** Admits one agent with a realm-unique name and a never-reused number (C9, C10, H27). */
export async function admit(
    tx: Tx,
    hamlet: string,
    given: {
        name: string;
        purpose: string;
        model?: string | null;
        character?: string;
        by: string;
        reason: string;
        at: number;
        roads?: readonly string[];
        task?: string | null;
    },
): Promise<Result<Admission, RecordRefusal>> {
    const name = tidyName(given.name);
    const valid = await availableName(tx, hamlet, name, "admit");
    if (!valid.ok) return valid;
    // A chief can overrule the mayor's admission requirements (H23).
    if (given.by === "mayor" && !given.purpose.trim())
        return refusal("admit", hamlet, { kind: "purpose-empty" });
    const record = await tx.doc(RecordDoc, hamlet, hamlet);
    const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
    const room = await roomFor(tx, hamlet, "admit");
    if (!room.ok) return room;
    const number = record.next++;
    const model =
        given.model && limits.models.includes(given.model) ? given.model : null;
    record.agents.push({
        number,
        name,
        purpose: given.purpose.trim(),
        since: given.at,
        model,
    });
    record.decisions.push({
        kind: "created",
        name,
        purpose: given.purpose.trim(),
        agent: number,
        at: given.at,
        by: given.by,
        reason: given.reason.trim(),
    });
    if (model)
        record.decisions.push({
            kind: "model",
            agent: number,
            model,
            at: given.at,
            by: given.by,
            reason: given.reason.trim(),
        });
    const key = agentKey({ hamlet, number });
    (await tx.doc(CharacterDoc, key, key)).text = given.character ?? "";
    const roadRefusals: { peer: string; refusal: RecordRefusal }[] = [];
    for (const peer of given.roads ?? []) {
        const road = await layRoad(tx, hamlet, name, peer, {
            by: given.by,
            reason: given.reason,
            at: given.at,
            task: given.task ?? null,
        });
        if (!road.ok) roadRefusals.push({ peer, refusal: road.error });
    }
    return ok({
        hamlet,
        number,
        ...(given.roads === undefined ? {} : { roadRefusals }),
    });
}

/** Checks realm uniqueness before a membership change; unchanged archived founder names are allowed. */
export async function availableName(
    tx: Tx,
    hamlet: string,
    name: string,
    operation: string,
    archive?: Archived,
    selector?: string,
): Promise<Result<void, RecordRefusal>> {
    if (name !== archive?.agent.name) {
        const checked = checkName(name, hamlet, operation);
        if (!checked.ok) return checked;
    }
    for (const h of liveHamlets(await tx.doc(RealmDoc))) {
        if (
            (await tx.doc(RecordDoc, h.id, h.id)).agents.some(
                (a) => a.name === name,
            )
        )
            return refusal(operation, hamlet, {
                kind: "name-taken",
                name,
                ...(archive ? { archive: selector ?? archive.id } : {}),
            });
    }
    return ok(undefined);
}

async function ordinary(
    tx: Tx,
    hamlet: string,
    name: string,
): Promise<Result<AgentRecord, RecordRefusal>> {
    const r = await tx.doc(RecordDoc, hamlet, hamlet);
    const agent = named(r, name);
    if (name === "mayor" || agent?.number === r.mayor)
        return refusal("depart", hamlet, { kind: "mayor-untouchable" });
    if (!agent) {
        const gone = r.archive.findLast(
            (a) => a.agent.name === name && a.back === null,
        );
        return gone
            ? refusal("depart", hamlet, {
                  kind: "agent-departed",
                  farewell: farewell(gone, r),
              })
            : refusal("depart", hamlet, { kind: "agent-missing", name });
    }
    return ok({ ...agent });
}
async function unprotected(
    tx: Tx,
    hamlet: string,
    agent: AgentRecord,
    given: { by: string; at: number; protectMs?: number },
): Promise<Result<void, RecordRefusal>> {
    if (given.by !== "mayor") return ok(undefined);
    const key = agentKey({ hamlet, number: agent.number });
    const i = owed(
        await tx.doc(InstructionsDoc, key, key),
        given.at,
        given.protectMs ??
            (await tx.doc(LimitsDoc, hamlet, hamlet)).leaving.protectedMinutes *
                60_000,
    );
    return i
        ? refusal("depart", hamlet, {
              kind: "protected",
              name: agent.name,
              chief: i.chief,
              text: i.text,
              done: i.done,
          })
        : ok(undefined);
}
/** Checks an ordinary agent and its reason before applying instruction protection (H5, H7). */
export async function departing(
    tx: Tx,
    hamlet: string,
    name: string,
    given: { by: string; at: number; protectMs?: number; reason?: string },
): Promise<Result<AgentRecord, RecordRefusal>> {
    const found = await ordinary(tx, hamlet, name);
    if (!found.ok) return found;
    if (given.reason !== undefined && !given.reason.trim())
        return refusal("depart", hamlet, { kind: "reason-empty" });
    const protection = await unprotected(tx, hamlet, found.value, given);
    return protection.ok ? found : protection;
}
/** Checks both merge participants, the proposed purpose and reason, then their chief tasks. */
export async function mergeable(
    tx: Tx,
    hamlet: string,
    given: {
        keep: string;
        absorb: string;
        by: string;
        purpose: string | null;
        reason: string;
        at: number;
        protectMs?: number;
    },
): Promise<Result<{ kept: AgentRecord; gone: AgentRecord }, RecordRefusal>> {
    if (
        given.by.startsWith("chief:") &&
        !isChief(await tx.doc(RealmDoc), hamlet, given.by.slice(6))
    )
        return refusal("merge", hamlet, { kind: "record-chief" });
    if (given.keep === given.absorb)
        return refusal("merge", hamlet, { kind: "merge-self" });
    const kept = await ordinary(tx, hamlet, given.keep);
    if (!kept.ok) return kept;
    const gone = await ordinary(tx, hamlet, given.absorb);
    if (!gone.ok) return gone;
    if (given.purpose !== null && !given.purpose.trim())
        return refusal("merge", hamlet, { kind: "merged-purpose-empty" });
    if (!given.reason.trim())
        return refusal("merge", hamlet, { kind: "reason-empty" });
    for (const agent of [kept.value, gone.value]) {
        const protection = await unprotected(tx, hamlet, agent, given);
        if (!protection.ok) return protection;
    }
    return ok({ kept: kept.value, gone: gone.value });
}

/** Archives an ordinary agent and lapses its waits, trust and grants in the same transaction. */
export async function depart(
    tx: Tx,
    hamlet: string,
    name: string,
    given: { by: string; reason: string; protectMs?: number; at: number } & (
        | { how: "removed" }
        | {
              how: "merged";
              into: number;
              purpose: string | null;
              notes: "joined" | "rewritten" | "plain";
              merge: Pick<
                  NonNullable<Archived["merge"]>,
                  "notesBefore" | "notesAfter" | "brought" | "clashes"
              >;
          }
    ),
): Promise<Result<Archived, RecordRefusal>> {
    let kept: AgentRecord | undefined;
    let agent: AgentRecord;
    if (given.how === "merged") {
        const r = await tx.doc(RecordDoc, hamlet, hamlet);
        const target = r.agents.find((a) => a.number === given.into);
        if (!target)
            return refusal("merge", hamlet, { kind: "proposed-kept-missing" });
        const allowed = await mergeable(tx, hamlet, {
            ...given,
            keep: target.name,
            absorb: name,
        });
        if (!allowed.ok) return allowed;
        kept = allowed.value.kept;
        agent = allowed.value.gone;
    } else {
        const found = await departing(tx, hamlet, name, given);
        if (!found.ok) return found;
        agent = found.value;
    }
    const r = await tx.doc(RecordDoc, hamlet, hamlet);
    let stamp = given.at;
    while (r.archive.some((a) => a.id === `${name}-${stamp}`)) stamp++;
    const entry: Archived = {
        id: `${name}-${stamp}`,
        agent,
        how: given.how,
        into: given.how === "merged" ? given.into : null,
        at: given.at,
        by: given.by,
        reason: given.reason.trim(),
        roads: roadsOf(r).filter(
            (r) => r.a === agent.number || r.b === agent.number,
        ),
        merge: null,
        back: null,
    };
    const ref = { hamlet, number: agent.number };
    await lapseAgent(tx, ref, given.at);
    await lapseTrust(tx, ref);
    for (const permission of ["network", "unconfined"] as const)
        await revoke(tx, ref, permission, given.by, given.at);
    r.agents = r.agents.filter((a) => a.number !== agent.number);
    r.roads = r.roads.filter(
        (r) => r.a !== agent.number && r.b !== agent.number,
    );
    if (given.how === "merged" && kept) {
        const inherited: NonNullable<Archived["merge"]>["inherited"] = [];
        for (const road of entry.roads) {
            const peer = road.a === agent.number ? road.b : road.a;
            if (peer === kept.number) continue;
            const existing = r.roads.find(
                (r) =>
                    (r.a === kept.number && r.b === peer) ||
                    (r.b === kept.number && r.a === peer),
            );
            const lastFrom =
                road.lastFrom === agent.number ? kept.number : road.lastFrom;
            if (!existing)
                r.roads.push({ ...road, a: kept.number, b: peer, lastFrom });
            else {
                existing.messages += road.messages;
                if (road.lastUsed > existing.lastUsed) {
                    existing.lastUsed = road.lastUsed;
                    existing.lastFrom = lastFrom;
                }
            }
            inherited.push({ peer, messages: road.messages, fresh: !existing });
        }
        entry.merge = { kept, ...given.merge, inherited, applied: false };
        const living = r.agents.find((a) => a.number === kept.number);
        if (!living) throw new Error("A checked merge lost its kept agent.");
        living.purpose = given.purpose?.trim() ?? kept.purpose;
        r.decisions.push({
            kind: "merged",
            at: given.at,
            by: given.by,
            reason: entry.reason,
            keep: kept.number,
            absorb: agent.number,
            purpose: living.purpose,
            notes: given.notes,
        });
    }
    r.archive.push(entry);
    if (given.how === "removed")
        r.decisions.push({
            kind: "removed",
            at: given.at,
            by: given.by,
            reason: entry.reason,
            agent: agent.number,
            name,
            archive: entry.id,
        });
    return ok(copyArchive(entry));
}

/** Restores membership and uncharged surviving roads, splitting inherited roads in reverse merge order (H20). */
export async function restore(
    tx: Tx,
    hamlet: string,
    archiveId: string,
    given: {
        name: string;
        by: string;
        reason: string;
        at: number;
        /** The chief's archive selector, retained in recovery advice. */
        selector?: string;
    },
): Promise<Result<Restored, RecordRefusal>> {
    const r = await tx.doc(RecordDoc, hamlet, hamlet);
    const at = r.archive.findIndex(
        (a) => a.id === archiveId && a.back === null,
    );
    const entry = r.archive[at];
    if (!entry)
        return refusal("restore", hamlet, {
            kind: "archive-missing",
            archive: archiveId,
        });
    if (entry.merge) {
        if (!entry.merge.applied)
            return refusal("restore", hamlet, { kind: "merge-unfinished" });
        const kept = r.agents.find(
            (a) => a.number === entry.merge?.kept.number,
        );
        if (!kept)
            return refusal("restore", hamlet, {
                kind: "kept-departed",
                kept: entry.merge.kept.name,
                absorbed: entry.agent.name,
            });
        const later = r.archive
            .slice(at + 1)
            .find(
                (a) => a.back === null && a.merge?.kept.number === kept.number,
            );
        if (later)
            return refusal("restore", hamlet, {
                kind: "later-merge",
                later: later.agent.name,
                kept: kept.name,
                absorbed: entry.agent.name,
            });
    }
    const valid = await availableName(
        tx,
        hamlet,
        given.name,
        "restore",
        entry,
        given.selector,
    );
    if (!valid.ok) return valid;
    const room = await roomFor(tx, hamlet, "restore");
    if (!room.ok) return room;
    if (entry.merge) {
        const kept = r.agents.find(
            (a) => a.number === entry.merge?.kept.number,
        );
        if (kept) kept.purpose = entry.merge.kept.purpose;
        for (const i of entry.merge.inherited) {
            const index = r.roads.findIndex(
                (road) =>
                    (road.a === entry.merge?.kept.number &&
                        road.b === i.peer) ||
                    (road.b === entry.merge?.kept.number && road.a === i.peer),
            );
            const road = r.roads[index];
            if (!road) continue;
            road.messages = Math.max(0, road.messages - i.messages);
            if (i.fresh && road.messages === 0) r.roads.splice(index, 1);
        }
        entry.merge.applied = false;
    }
    const agent = { ...entry.agent, name: given.name };
    r.agents.push(agent);
    for (const road of entry.roads)
        if (
            [road.a, road.b].every(
                (end) =>
                    end === "office" || r.agents.some((a) => a.number === end),
            ) &&
            !r.roads.some(
                (r) =>
                    (r.a === road.a && r.b === road.b) ||
                    (r.a === road.b && r.b === road.a),
            )
        )
            r.roads.push({ ...road });
    entry.back = given.name;
    r.decisions.push({
        kind: "restored",
        at: given.at,
        by: given.by,
        reason: given.reason,
        agent: entry.agent.number,
        name: given.name,
        archive: entry.id,
    });
    const peers = roadsOf(r)
        .filter((road) => road.a === agent.number || road.b === agent.number)
        .map((road) => endName(r, road.a === agent.number ? road.b : road.a));
    const built = roadsOf(r).filter(
        (road) => road.a !== "office" && road.b !== "office",
    ).length;
    const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
    const excess =
        built > limits.mostRoads
            ? ` The hamlet now has ${built} roads between agents, more than the ${limits.mostRoads} your limits allow. They all work; the mayor can build no more until you raise mostRoads in the limits.`
            : "";
    const keptName = entry.merge
        ? (r.agents.find((a) => a.number === entry.merge?.kept.number)?.name ??
          entry.merge.kept.name)
        : "";
    const text = entry.merge
        ? `Split \`${given.name}\` from \`${keptName}\`. \`${given.name}\` has its house as it was before the merge, and \`${keptName}\` its old purpose and whatever it has learned since. \`${given.name}\`'s roads lead to: ${peers.join(", ")}.${excess}`
        : `Restored \`${given.name}\` with its house as it was. Its roads lead to: ${peers.join(", ")}.${excess}`;
    return ok({ agent, archive: copyArchive(entry), text });
}

/** A detached archive receipt, safe beyond the transaction's lifetime. */
export function copyArchive(entry: Archived): Archived {
    return {
        ...entry,
        agent: { ...entry.agent },
        roads: entry.roads.map((r) => ({ ...r })),
        merge: entry.merge
            ? {
                  ...entry.merge,
                  kept: { ...entry.merge.kept },
                  brought: entry.merge.brought.map((p) => ({ ...p })),
                  clashes: [...entry.merge.clashes],
                  inherited: entry.merge.inherited.map((r) => ({ ...r })),
              }
            : null,
    };
}

/** Takes edit and command trust back from every conversation of a departing agent. */
export async function lapseTrust(tx: Tx, agent: AgentRef): Promise<void> {
    const key = agentKey(agent);
    for (const id of (await tx.doc(AgentConversationsDoc, key, key)).ids)
        await resetTrust(tx, await tx.doc(WithDoc, id));
}

/** Seeds a newly founded hamlet's record with its first office holder. */
export async function foundRecord(
    tx: Tx,
    hamlet: string,
    given: { name: string; purpose: string; at: number },
): Promise<Result<void, RecordRefusal>> {
    const available = await availableName(tx, hamlet, given.name, "found");
    if (!available.ok) return available;
    const r = await tx.doc(RecordDoc, hamlet, hamlet);
    if (
        r.next !== 1 ||
        r.agents.length !== 1 ||
        r.decisions.length ||
        r.archive.length
    )
        throw new Error("Founding requires a fresh record.");
    r.agents = [
        {
            number: 0,
            name: given.name,
            purpose: given.purpose.trim(),
            since: given.at,
            model: null,
        },
    ];
    return ok(undefined);
}

/** Checks living agents other than the mayor against the chief's current cap (H27). */
export async function roomFor(
    tx: Tx,
    hamlet: string,
    operation: string,
): Promise<Result<void, RecordRefusal>> {
    const r = await tx.doc(RecordDoc, hamlet, hamlet);
    const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
    return r.agents.filter((a) => a.number !== r.mayor).length >=
        limits.mostAgents
        ? refusal(operation, hamlet, {
              kind: "hamlet-full",
              most: limits.mostAgents,
          })
        : ok(undefined);
}
/** Checks a move's office, capacity and unsplit-archive constraints in its transaction. */
export async function moving(
    tx: Tx,
    from: AgentRef,
    to: string,
): Promise<Result<AgentRecord, RecordRefusal>> {
    const old = await tx.doc(RecordDoc, from.hamlet, from.hamlet);
    if (old.mayor === from.number)
        return refusal("move", from.hamlet, { kind: "move-mayor" });
    const agent = old.agents.find((a) => a.number === from.number);
    if (!agent)
        return refusal("move", from.hamlet, {
            kind: "agent-missing",
            name: String(from.number),
        });
    const room = await roomFor(tx, to, "move");
    if (!room.ok) return room;
    const merged = old.archive.find(
        (entry) =>
            entry.back === null && entry.merge?.kept.number === from.number,
    );
    if (merged)
        return refusal("agree-move", from.hamlet, {
            kind: "unsplit-merge",
            agent: from,
            name: agent.name,
            absorbed: merged.agent.name,
            archive: merged.id,
            recovery: "split-or-erase",
        });
    return ok({ ...agent });
}
/** Moves an ordinary member's identity to its next number in a receiving hamlet (C9, H14). */
export async function moveMember(
    tx: Tx,
    from: AgentRef,
    to: string,
    given: { by: string; reason: string; at: number },
): Promise<Result<AgentRef, RecordRefusal>> {
    const old = await tx.doc(RecordDoc, from.hamlet, from.hamlet);
    const target = await tx.doc(RecordDoc, to, to);
    const permitted = await moving(tx, from, to);
    if (!permitted.ok) return permitted;
    const agent = permitted.value;
    const number = target.next++;
    target.agents.push({ ...agent, number });
    old.agents = old.agents.filter((a) => a.number !== from.number);
    old.roads = old.roads.filter(
        (r) => r.a !== from.number && r.b !== from.number,
    );
    old.decisions.push({
        kind: "moved_out",
        agent: from.number,
        name: agent.name,
        hamlet: to,
        at: given.at,
        by: given.by,
        reason: given.reason,
    });
    target.decisions.push({
        kind: "moved_in",
        agent: number,
        name: agent.name,
        hamlet: from.hamlet,
        at: given.at,
        by: given.by,
        reason: given.reason,
    });
    return ok({ hamlet: to, number });
}
