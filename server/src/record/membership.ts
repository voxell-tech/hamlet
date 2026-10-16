import type { Tx } from "@earendil-works/pi-durable";
import { AgentConversationsDoc, WithDoc } from "../chats/conversations.ts";
import { InstructionsDoc, owed } from "../chats/instructions.ts";
import { revoke } from "../gate/grants.ts";
import { LimitsDoc } from "../limits/rules.ts";
import { resetTrust } from "../oversight/asked.ts";
import { lapseAgent } from "../oversight/waits.ts";
import { CharacterDoc } from "../prompt/character.ts";
import { liveHamlets, RealmDoc } from "../realm/places.ts";
import { ok, type Result } from "../result.ts";
import {
    type AgentRecord,
    type AgentRef,
    type Archived,
    agentKey,
    RecordDoc,
} from "./document.ts";
import { farewell, named, tidyName } from "./names.ts";
import { type RecordRefusal, refusal } from "./refusals.ts";
import { roadsOf } from "./roads.ts";

/** A restored identity and the archive whose house must be brought back. */
export type Restored = { agent: AgentRecord; archive: Archived };

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
    },
): Promise<Result<AgentRef, RecordRefusal>> {
    const name = tidyName(given.name);
    const valid = await availableName(tx, hamlet, name, "admit");
    if (!valid.ok) return valid;
    if (!given.purpose.trim())
        return refusal("admit", hamlet, { kind: "purpose-empty" });
    const record = await tx.doc(RecordDoc, hamlet, hamlet);
    const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
    if (record.agents.length >= limits.mostAgents)
        return refusal("admit", hamlet, {
            kind: "hamlet-full",
            most: limits.mostAgents,
        });
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
    return ok({ hamlet, number });
}

/** Checks realm uniqueness before a membership change; unchanged archived founder names are allowed. */
export async function availableName(
    tx: Tx,
    hamlet: string,
    name: string,
    operation: string,
    archive?: Archived,
): Promise<Result<void, RecordRefusal>> {
    if (name !== archive?.agent.name) {
        if (!/^[a-z-]{1,24}$/.test(name))
            return refusal(operation, hamlet, { kind: "name-format" });
        if (["mayor", "founder", "all"].includes(name))
            return refusal(operation, hamlet, { kind: "name-reserved", name });
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
                ...(archive ? { archive: archive.id } : {}),
            });
    }
    return ok(undefined);
}

/** Finds an ordinary living agent and enforces the mayor's instruction protection (H5, H7). */
export async function departing(
    tx: Tx,
    hamlet: string,
    name: string,
    given: { by: string; at: number; protectMs?: number },
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
    if (given.by === "mayor") {
        const key = agentKey({ hamlet, number: agent.number });
        const i = owed(
            await tx.doc(InstructionsDoc, key, key),
            given.at,
            given.protectMs ??
                (await tx.doc(LimitsDoc, hamlet, hamlet)).leaving
                    .protectedMinutes * 60_000,
        );
        if (i)
            return refusal("depart", hamlet, {
                kind: "protected",
                name,
                chief: i.chief,
                text: i.text,
                done: i.done,
            });
    }
    return ok({ ...agent });
}

/** Archives an ordinary agent and lapses its waits, trust and grants in the same transaction. */
export async function depart(
    tx: Tx,
    hamlet: string,
    name: string,
    given: {
        by: string;
        reason: string;
        how: "removed" | "merged";
        into?: number;
        protectMs?: number;
        at: number;
    },
): Promise<Result<Archived, RecordRefusal>> {
    const found = await departing(tx, hamlet, name, given);
    if (!found.ok) return found;
    if (!given.reason.trim())
        return refusal("depart", hamlet, { kind: "reason-empty" });
    const agent = found.value;
    const r = await tx.doc(RecordDoc, hamlet, hamlet);
    let stamp = given.at;
    while (r.archive.some((a) => a.id === `${name}-${stamp}`)) stamp++;
    const entry: Archived = {
        id: `${name}-${stamp}`,
        agent,
        how: given.how,
        into: given.into ?? null,
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
    const key = agentKey(ref);
    for (const id of (await tx.doc(AgentConversationsDoc, key, key)).ids)
        await resetTrust(tx, await tx.doc(WithDoc, id));
    for (const permission of ["network", "unconfined"] as const)
        await revoke(tx, ref, permission, given.by, given.at);
    r.agents = r.agents.filter((a) => a.number !== agent.number);
    r.roads = r.roads.filter(
        (r) => r.a !== agent.number && r.b !== agent.number,
    );
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
    given: { name: string; by: string; reason: string; at: number },
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
    const valid = await availableName(tx, hamlet, given.name, "restore", entry);
    if (!valid.ok) return valid;
    const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
    if (r.agents.length >= limits.mostAgents)
        return refusal("restore", hamlet, {
            kind: "hamlet-full",
            most: limits.mostAgents,
        });
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
    return ok({ agent, archive: copyArchive(entry) });
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
