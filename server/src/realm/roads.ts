import type { Context } from "@earendil-works/chord";
import type { DocumentReader } from "@earendil-works/pi-durable";
import type { ChatId } from "../chats/conversations.ts";
import type { Home } from "../home.ts";
import type { Address, State } from "../protocol.ts";
import {
    type AgentRef,
    type Archived,
    type End,
    type HamletRecord,
    HOME,
    MAYOR,
    named,
    RecordDoc,
    type RoadRecord,
} from "../record.ts";
import { err, ok, type Result } from "../result.ts";
import { type ChiefRefusal, chiefAccess, chiefRefusalText } from "./access.ts";
import {
    liveHamlets,
    type Realm,
    RealmDoc,
    type RealmEnd,
    type RealmRoad,
} from "./places.ts";

/** An unavailable agent address, retaining a departed agent's farewell verbatim. */
export type AddressFailure =
    | ChiefRefusal["error"]
    | ({ operation: string; hamlet: string; agent: string } & (
          | { kind: "agent-missing"; retrySafe: true }
          | { kind: "agent-gone"; farewell: string; retrySafe: false }
      ));

/** Chief-facing wording for an unavailable agent address. */
export function addressFailureText(failure: AddressFailure): string {
    switch (failure.kind) {
        case "agent-missing":
            return `no agent named \`${failure.agent}\``;
        case "agent-gone":
            return failure.farewell;
        case "hamlet-missing":
        case "hamlet-removed":
        case "not-chief":
            return chiefRefusalText({ ok: false, error: failure });
    }
    const unhandled: never = failure;
    throw new Error(`Unclassified address failure: ${unhandled}`);
}

/** Resolves the office and checks the receiving hamlet's chief before exposing a living chat. */
export async function livingAgent(
    home: Home,
    address: Address | { from: string; text: string },
    operation: string,
    context: Context,
): Promise<Result<{ ref: AgentRef; chat: ChatId }, AddressFailure>> {
    const hamlet = "from" in address ? address.from : address.hamlet;
    const agent = "from" in address ? address.text : address.agent;
    if (!("from" in address)) {
        const access = await chiefAccess(home, hamlet, operation, context);
        if (!access.ok) return access;
    }
    const target = await resolveAddress(
        home.harness,
        hamlet,
        "from" in address ? address.text : `${agent}@${hamlet}`,
        context,
    );
    const facts = { operation, hamlet, agent };
    if (!target.ok) {
        if (target.error.kind === "departed")
            return err({
                ...facts,
                kind: "agent-gone",
                farewell: target.error.farewell,
                retrySafe: false,
            });
        return err({ ...facts, kind: "agent-missing", retrySafe: true });
    }
    if ("from" in address) {
        const access = await chiefAccess(
            home,
            target.value.hamlet,
            operation,
            context,
        );
        if (!access.ok) return access;
    }
    return ok({
        ref: target.value.ref,
        chat: { agent: target.value.ref, chief: home.chief },
    });
}

/** A living address in a client's received state; archived agents never resolve. */
export function findInState(
    state: State,
    address: { agent: string; hamlet?: string },
): AgentRef | undefined {
    const hamlet =
        address.hamlet ?? (address.agent === MAYOR ? HOME : undefined);
    return state.agents.find(
        (agent) =>
            !agent.left &&
            (hamlet === undefined || agent.ref.hamlet === hamlet) &&
            (address.agent === MAYOR
                ? agent.mayor
                : agent.name === address.agent),
    )?.ref;
}

/** A tool address refused before any work is admitted. */
export type ResolveFailure =
    | { kind: "malformed" }
    | { kind: "unknown"; address: string }
    | { kind: "departed"; farewell: string };

/** Resolves a name across the realm, keeping an unqualified office local to its sender. */
export async function resolveAddress(
    read: DocumentReader,
    from: string,
    text: string,
    context: Context,
): Promise<
    Result<{ ref: AgentRef; address: string; hamlet: string }, ResolveFailure>
> {
    const [name, , extra] = text.split("@");
    if (!name || extra !== undefined) return err({ kind: "malformed" });
    const target = await resolve(read, from, text, context);
    if (!target) return err({ kind: "unknown", address: text });
    if ("gone" in target)
        return err({ kind: "departed", farewell: target.gone });
    return ok(target);
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
