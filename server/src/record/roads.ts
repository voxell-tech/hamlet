import type { End, HamletRecord, RoadRecord } from "./document.ts";
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
