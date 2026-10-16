import type { Context } from "@earendil-works/chord";
import type { DocumentReader, Tx } from "@earendil-works/pi-durable";
import type { ChatId } from "../chats/conversations.ts";
import type { Home } from "../home.ts";
import type { Address, State } from "../protocol.ts";
import {
    type AgentRef,
    farewell,
    type HamletRecord,
    HOME,
    MAYOR,
    named,
    RecordDoc,
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

/** A living name or office, resolved in the sender's realm. */
export type Resolved = { ref: AgentRef; address: string; hamlet: string };

/** Tool-facing wording for a refused address, with the original spelling when malformed. */
export function resolveFailureText(
    failure: ResolveFailure,
    text: string,
): string {
    switch (failure.kind) {
        case "malformed":
            return `no agent named \`${text}\``;
        case "unknown":
            return `no agent named \`${failure.address}\``;
        case "departed":
            return failure.farewell;
    }
    const unhandled: never = failure;
    throw new Error(`Unclassified resolved address failure: ${unhandled}`);
}

/** Resolves current transaction records or committed snapshots, keeping the unqualified office local. */
export async function resolveAddress(
    read: DocumentReader | Tx,
    from: string,
    address: string,
    context: Context,
): Promise<Result<Resolved, ResolveFailure>> {
    const [name, place, extra] = address.split("@");
    if (!name || extra !== undefined || place === "")
        return err({ kind: "malformed" });
    const realm =
        "doc" in read
            ? await read.doc(RealmDoc)
            : await read.snapshot(RealmDoc, context);
    const places = liveHamlets(realm).filter((hamlet) =>
        place === undefined
            ? name !== MAYOR || hamlet.id === from
            : hamlet.id === place,
    );
    const records = await Promise.all(
        places.map(async (hamlet) => ({
            hamlet: hamlet.id,
            record:
                "doc" in read
                    ? await read.doc(RecordDoc, hamlet.id, hamlet.id)
                    : await read.snapshot(RecordDoc, hamlet.id, context),
        })),
    );
    for (const { hamlet, record } of records) {
        if (!record) continue;
        const agent = named(record, name);
        if (agent)
            return ok({
                ref: { hamlet, number: agent.number },
                hamlet,
                address:
                    agent.number === record.mayor
                        ? hamlet === from
                            ? MAYOR
                            : `${MAYOR}@${hamlet}`
                        : agent.name,
            });
    }
    for (const { record } of records) {
        const gone = record?.archive.findLast(
            (entry) => entry.agent.name === name && entry.back === null,
        );
        if (gone && record)
            return err({ kind: "departed", farewell: farewell(gone, record) });
    }
    return err({ kind: "unknown", address });
}

export { carryRoad, farewell, hasRoad, level, roadsOf } from "../record.ts";
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
