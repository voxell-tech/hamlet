import type { Context } from "@earendil-works/chord";
import type { DocumentReader } from "@earendil-works/pi-durable";
import {
    type AgentRef,
    farewell,
    type HamletRecord,
    MAYOR,
    named,
    RecordDoc,
} from "../record.ts";
import {
    liveHamlets,
    type Realm,
    RealmDoc,
    type RealmEnd,
    type RealmRoad,
} from "./places.ts";

/** An unavailable agent address, retaining a departed agent's farewell verbatim. */
export type AddressFailure =
    | { kind: "agent-missing"; agent: string }
    | { kind: "agent-gone"; farewell: string };

/** Chief-facing wording for an unavailable agent address. */
export function addressFailureText(failure: AddressFailure): string {
    switch (failure.kind) {
        case "agent-missing":
            return `no agent named \`${failure.agent}\``;
        case "agent-gone":
            return failure.farewell;
    }
    const unhandled: never = failure;
    throw new Error(`Unclassified address failure: ${unhandled}`);
}

export { farewell } from "../record.ts";
/** An address resolved in the sender's realm, or a departed agent's farewell. */
export type Resolved =
    | { ref: AgentRef; address: string; hamlet: string }
    | { gone: string };

/** A name or office resolved across living hamlets. */
export async function resolve(
    read: DocumentReader,
    from: string,
    address: string,
    context: Context,
): Promise<Resolved | undefined> {
    const [name, place, extra] = address.split("@");
    if (extra !== undefined) return undefined;
    if (!name) return undefined;
    const realm = await read.snapshot(RealmDoc, context);
    const places = liveHamlets(realm).filter((h) =>
        place === undefined ? name !== MAYOR || h.id === from : h.id === place,
    );
    for (const h of places) {
        const record = await read.snapshot(RecordDoc, h.id, context);
        if (!record) continue;
        const agent = named(record, name);
        if (agent)
            return {
                ref: { hamlet: h.id, number: agent.number },
                hamlet: h.id,
                address:
                    agent.number === record.mayor
                        ? h.id === from
                            ? MAYOR
                            : `${MAYOR}@${h.id}`
                        : agent.name,
            };
    }
    for (const h of places) {
        const record = await read.snapshot(RecordDoc, h.id, context);
        const gone = record?.archive.findLast(
            (a) => a.agent.name === name && a.back === null,
        );
        if (gone && record) return { gone: farewell(gone, record) };
    }
    return undefined;
}

export { carryRoad, hasRoad, level, roadsOf } from "../record.ts";
/** Cross-hamlet roads with their ends resolved to living agents. */
export function realmRoads(
    realm: Readonly<Realm>,
    records: Readonly<Record<string, Readonly<HamletRecord>>>,
): (Omit<RealmRoad, "a" | "b"> & { a: AgentRef; b: AgentRef })[] {
    const live = liveHamlets(realm);
    const end = (e: RealmEnd): AgentRef | undefined => {
        const h = "office" in e ? e.office : e.hamlet;
        const record = records[h];
        if (!live.some((p) => p.id === h) || !record) return undefined;
        const number = "office" in e ? record.mayor : e.number;
        return record.agents.some((a) => a.number === number)
            ? { hamlet: h, number }
            : undefined;
    };
    const roads = [...realm.roads];
    for (let i = 0; i < live.length; i++)
        for (const b of live.slice(i + 1)) {
            const a = live[i];
            if (!a) continue;
            roads.push(
                realm.offices.find(
                    (r) =>
                        "office" in r.a &&
                        "office" in r.b &&
                        [r.a.office, r.b.office].includes(a.id) &&
                        [r.a.office, r.b.office].includes(b.id),
                ) ?? {
                    a: { office: a.id },
                    b: { office: b.id },
                    agreed: [a.id, b.id],
                    reason: "office",
                    messages: 0,
                    lastFrom: null,
                    lastUsed: 0,
                },
            );
        }
    return roads.flatMap((r) => {
        const a = end(r.a),
            b = end(r.b);
        return a &&
            b &&
            a.hamlet !== b.hamlet &&
            r.agreed.includes(a.hamlet) &&
            r.agreed.includes(b.hamlet)
            ? [{ ...r, a, b }]
            : [];
    });
}

/** One message carried by a cross-hamlet road. */
export function carryRealmRoad(
    road: RealmRoad,
    from: RealmEnd,
    at: number,
): void {
    road.messages++;
    road.lastFrom = from;
    road.lastUsed = at;
}
