import type { Context } from "@earendil-works/chord";
import { type DocumentReader, defineDoc } from "@earendil-works/pi-durable";
import { type AgentRef, HOME, MAYOR } from "./hamlet.ts";
import {
    type Archived,
    type End,
    type HamletRecord,
    named,
    RecordDoc,
    type RoadRecord,
} from "./record.ts";

/** An agent or a hamlet's office as a lasting road end. */
export type RealmEnd = AgentRef | { office: string };
/** A hamlet's place in the realm. */
export type HamletPlace = {
    id: string;
    title: string | null;
    at: { x: number; z: number } | null;
    /** Empty means the human the home belongs to. */
    chiefs: string[];
    from: { hamlet: string; vote: number } | null;
    founded: number;
    removed: { at: number; by: string; reason: string } | null;
    erased: boolean;
};
/** An agreed road between hamlets. */
export type RealmRoad = {
    a: RealmEnd;
    b: RealmEnd;
    agreed: string[];
    reason: string;
    messages: number;
    lastFrom: RealmEnd | null;
    lastUsed: number;
};
/** The realm's places, agreements and unfinished house move. */
export type Realm = {
    hamlets: HamletPlace[];
    roads: RealmRoad[];
    offices: RealmRoad[];
    moves: {
        agent: AgentRef;
        to: string;
        agreed: {
            hamlet: string;
            keep: number[];
            reason: string;
            at: number;
        }[];
    }[];
    removals: { hamlet: string; by: string; reason: string; at: number }[];
    pending: { from: AgentRef; to: AgentRef } | null;
};
/** The home's realm (H6). */
export const RealmDoc = defineDoc<Realm>({
    kind: "hamlet.realm",
    version: 2,
    migrate: (value) => {
        const realm = value as unknown as Realm;
        return {
            ...realm,
            hamlets: realm.hamlets.map((h) =>
                h.id === HOME && h.chiefs.length === 1
                    ? { ...h, chiefs: [] }
                    : h,
            ),
        };
    },
    scope: "session",
    initial: () => ({
        hamlets: [],
        roads: [],
        offices: [],
        moves: [],
        removals: [],
        pending: null,
    }),
});
/** The realm's living hamlets. */
export function liveHamlets(realm: Readonly<Realm> | undefined): HamletPlace[] {
    return realm?.hamlets.filter((h) => h.removed === null && !h.erased) ?? [];
}
/** The humans told about a hamlet, including the home chief for an empty list. */
export function chiefsOf(
    realm: Readonly<Realm> | undefined,
    hamlet: string,
    homeChief: string,
): readonly string[] {
    const place = realm?.hamlets.find((h) => h.id === hamlet);
    return place ? (place.chiefs.length ? place.chiefs : [homeChief]) : [];
}
/** A chief's authority over a living hamlet (C7). */
export function isChief(
    realm: Readonly<Realm>,
    hamlet: string,
    chief: string,
): boolean {
    return liveHamlets(realm).some(
        (h) =>
            h.id === hamlet &&
            (h.chiefs.length === 0 || h.chiefs.includes(chief)),
    );
}
/** A departed agent's refusal (H28). */
export function farewell(
    entry: Archived,
    record: Readonly<HamletRecord>,
): string {
    const name = entry.agent.name;
    const by =
        entry.by === MAYOR
            ? "the mayor"
            : entry.by.startsWith("chief:")
              ? `the chief ${entry.by.slice(6)}`
              : entry.by;
    if (entry.how === "removed")
        return `\`${name}\` was removed from the hamlet by ${by} ("${entry.reason}"). Its house is archived and its roads went with it; only a chief can restore it.`;
    const kept =
        record.agents.find((a) => a.number === entry.into)?.name ??
        entry.merge?.kept.name ??
        "the kept agent";
    return `\`${name}\` was merged into \`${kept}\` by ${by} ("${entry.reason}"). \`${kept}\` has its notes, skills and roads; talk to \`${kept}\`.`;
}
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
/** The hamlet's active roads, including implied roads to the office. */
export function roadsOf(record: Readonly<HamletRecord>): RoadRecord[] {
    const lives = (end: End) =>
        end === "office" ||
        record.agents.some((a) => a.number === end && end !== record.mayor);
    const roads = record.roads
        .filter((r) => lives(r.a) && lives(r.b))
        .map((r) => ({ ...r }));
    for (const agent of record.agents)
        if (
            agent.number !== record.mayor &&
            !roads.some((r) => joins(r, "office", agent.number))
        )
            roads.push({
                a: "office",
                b: agent.number,
                messages: 0,
                lastFrom: null,
                lastUsed: 0,
                since: agent.since,
                by: "office",
            });
    return roads;
}
function joins(r: { a: End; b: End }, a: End, b: End): boolean {
    return (r.a === a && r.b === b) || (r.a === b && r.b === a);
}
/** A road between two living agents of the hamlet. */
export function hasRoad(
    record: Readonly<HamletRecord>,
    a: number,
    b: number,
): boolean {
    return (
        a !== b &&
        roadsOf(record).some((r) =>
            joins(
                r,
                a === record.mayor ? "office" : a,
                b === record.mayor ? "office" : b,
            ),
        )
    );
}
/** One message carried by a local road. */
export function carryRoad(
    record: HamletRecord,
    from: number,
    to: number,
    at: number,
): void {
    const a = from === record.mayor ? "office" : from,
        b = to === record.mayor ? "office" : to;
    let road = record.roads.find((r) => joins(r, a, b));
    if (!road && hasRoad(record, from, to)) {
        road = roadsOf(record).find((r) => joins(r, a, b));
        if (road) record.roads.push(road);
    }
    if (!road) throw new Error("There is no road between those agents.");
    road.messages++;
    road.lastFrom = a;
    road.lastUsed = at;
}
/** A road's drawn level at the given time (H3). */
export function level(
    road: { messages: number; lastUsed: number },
    now: number,
    growsOverDays: number,
): "overgrown" | "dirt" | "paved" | "rail" {
    return road.lastUsed > 0 &&
        Math.max(0, now - road.lastUsed) > growsOverDays * 86400000
        ? "overgrown"
        : road.messages < 3
          ? "dirt"
          : road.messages < 10
            ? "paved"
            : "rail";
}
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
