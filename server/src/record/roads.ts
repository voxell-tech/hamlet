import type { Tx } from "@earendil-works/pi-durable";
import { localDay } from "../home/clock.ts";
import { LimitsDoc } from "../limits/rules.ts";
import { ok, type Result } from "../result.ts";
import { chargeRoad, LedgerDoc, roadAfford } from "../spend/ledger.ts";
import { add, zero } from "../spend/money.ts";
import type { End, HamletRecord, RoadRecord } from "./document.ts";
import { RecordDoc } from "./document.ts";
import { named } from "./names.ts";
import { type RecordRefusal, refusal } from "./refusals.ts";
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
        if (road) {
            record.roads.push(road);
            road = record.roads.find((r) => joins(r, a, b));
        }
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

/** Builds and charges a non-office road, appending its reason with the change (H3). */
export async function layRoad(
    tx: Tx,
    hamlet: string,
    a: string,
    b: string,
    given: { by: string; reason: string; at: number; task?: string | null },
): Promise<Result<RoadRecord, RecordRefusal>> {
    const r = await tx.doc(RecordDoc, hamlet, hamlet);
    const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
    const aa = named(r, a),
        bb = named(r, b);
    if (aa?.number === r.mayor || bb?.number === r.mayor)
        return refusal("lay-road", hamlet, { kind: "office-road" });
    if (!aa)
        return refusal("lay-road", hamlet, {
            kind: "agent-missing",
            name: a,
            local: true,
        });
    if (!bb)
        return refusal("lay-road", hamlet, {
            kind: "agent-missing",
            name: b,
            local: true,
        });
    if (aa.number === bb.number)
        return refusal("lay-road", hamlet, { kind: "road-self" });
    if (r.roads.some((r) => joins(r, aa.number, bb.number)))
        return refusal("lay-road", hamlet, { kind: "road-exists", a, b });
    const built = roadsOf(r).filter(
        (r) => r.a !== "office" && r.b !== "office",
    ).length;
    if (built >= limits.mostRoads)
        return refusal("lay-road", hamlet, { kind: "roads-full", most: built });
    const books = { hamlet: zero(), agents: {}, task: zero() };
    const today = new Date(given.at);
    const date = new Date(
        today.getFullYear(),
        today.getMonth(),
        limits.budget.period === "day" ? today.getDate() : 1,
    );
    while (date <= today) {
        const day = localDay(date.getTime());
        for (const line of (await tx.doc(LedgerDoc, `${hamlet}/${day}`, day))
            .lines) {
            if (line.what === "reset") books.hamlet = zero();
            else add(books.hamlet, line);
        }
        date.setDate(date.getDate() + 1);
    }
    const price = roadAfford(books, limits);
    if (price)
        return refusal("lay-road", hamlet, {
            kind: "road-budget",
            text: price,
        });
    await chargeRoad(tx, hamlet, {
        task: given.task ?? null,
        agent: r.mayor,
        at: given.at,
        limits,
    });
    const road: RoadRecord = {
        a: aa.number,
        b: bb.number,
        messages: 0,
        lastFrom: null,
        lastUsed: 0,
        since: given.at,
        by: given.by,
    };
    r.roads.push(road);
    r.decisions.push({
        kind: "road_built",
        a: aa.number,
        b: bb.number,
        at: given.at,
        by: given.by,
        reason: given.reason.trim(),
    });
    return ok({ ...road });
}

/** Takes up a built road without refunding its cost; office roads cannot be removed. */
export async function takeUpRoad(
    tx: Tx,
    hamlet: string,
    a: string,
    b: string,
    given: { by: string; reason: string; at: number },
): Promise<Result<RoadRecord, RecordRefusal>> {
    const r = await tx.doc(RecordDoc, hamlet, hamlet);
    const aa = named(r, a),
        bb = named(r, b);
    if (aa?.number === r.mayor || bb?.number === r.mayor)
        return refusal("take-up-road", hamlet, { kind: "office-road" });
    const index = r.roads.findIndex(
        (r) => aa && bb && joins(r, aa.number, bb.number),
    );
    const road = r.roads[index];
    if (!road)
        return refusal("take-up-road", hamlet, { kind: "road-missing", a, b });
    r.roads.splice(index, 1);
    r.decisions.push({
        kind: "road_removed",
        a: road.a,
        b: road.b,
        at: given.at,
        by: given.by,
        reason: given.reason.trim(),
    });
    return ok({ ...road });
}
