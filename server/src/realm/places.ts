import { defineDoc } from "@earendil-works/pi-durable";
import { type AgentRef, HOME } from "../record/document.ts";

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
