import type { ConversationId } from "@earendil-works/pi-durable";
import { chatState, chatStateDocs } from "../chats.ts";
import { modeWords, WorkDoc } from "../environment.ts";
import { GrantsDoc, grantsOf } from "../gate.ts";
import { conversationViews, PeersDoc } from "../letters.ts";
import { LimitsDoc, limitsOf } from "../limits/rules.ts";
import type { Agent, Grant, Hamlet, Road, State } from "../protocol.ts";
import {
    agentKey,
    type HamletRecord,
    named,
    RecordDoc,
    VotesDoc,
    voteViews,
} from "../record.ts";
import { choose, type Settings } from "../settings.ts";
import { booksOf, LedgerDoc } from "../spend/ledger.ts";
import { money } from "../spend/money.ts";
import { reportOf } from "../spend.ts";
import { activityOf, type Work } from "../state/activity.ts";
import type { View } from "../state.ts";
import { liveHamlets, RealmDoc } from "./places.ts";
import { farewell, level, realmRoads, roadsOf } from "./roads.ts";

/** Realm entities and the record/session identities needed by the waits view. */
export type RealmState = Required<
    Pick<State, "hamlets" | "agents" | "grants" | "closed">
> & {
    records: Record<string, Readonly<HamletRecord>>;
    currentChats: Map<string, Record<string, number>>;
};

/** Realm state rebuilt only for documents owned by its supplying modules. */
export const realmView: View<
    RealmState,
    { activity: ReadonlyMap<ConversationId, Work>; settings: Settings }
> = {
    docs: [
        RealmDoc.definition,
        RecordDoc.definition,
        WorkDoc.definition,
        LimitsDoc.definition,
        GrantsDoc.definition,
        LedgerDoc.definition,
        VotesDoc.definition,
        PeersDoc.definition,
        ...chatStateDocs,
    ],
    async build(home, context, folded) {
        const read = home.harness;
        const realm = await read.snapshot(RealmDoc, context);
        const places = liveHamlets(realm);
        const records: Record<string, Readonly<HamletRecord>> = {};
        for (const h of places)
            records[h.id] =
                (await read.snapshot(RecordDoc, h.id, context)) ??
                RecordDoc.definition.initial(h.id);
        const settings = folded.settings;
        const activity = folded.activity;
        const hamlets: Hamlet[] = [],
            agents: Agent[] = [],
            grants: Grant[] = grantsOf(
                await read.snapshot(GrantsDoc, context),
            ).map((g) => ({ ...g, source: "grant" }));
        const currentChats = new Map<string, Record<string, number>>();
        const crossRoads = realm ? realmRoads(realm, records) : [];
        for (const place of places) {
            const hamlet = place.id;
            const record = records[hamlet];
            if (!record) continue;
            const limits = limitsOf(
                await read.snapshot(LimitsDoc, hamlet, context),
            );
            const work =
                (await read.snapshot(WorkDoc, hamlet, context)) ??
                WorkDoc.definition.initial(hamlet);
            const books = await booksOf(
                home,
                hamlet,
                limits.budget.period,
                null,
                context,
            );
            const refOf = (end: number | "office") => ({
                hamlet,
                number: end === "office" ? record.mayor : end,
            });
            const roads: Road[] = roadsOf(record).map((r) => ({
                a: refOf(r.a),
                b: refOf(r.b),
                messages: r.messages,
                lastFrom: r.lastFrom === null ? null : refOf(r.lastFrom),
                level: level(r, home.now(), limits.road.growsOverDays),
            }));
            for (const road of crossRoads.filter(
                (r) => r.a.hamlet === hamlet || r.b.hamlet === hamlet,
            )) {
                const end = road.lastFrom;
                const lastFrom =
                    end === null
                        ? null
                        : "office" in end
                          ? {
                                hamlet: end.office,
                                number: records[end.office]?.mayor ?? 0,
                            }
                          : end;
                roads.push({
                    a: road.a,
                    b: road.b,
                    messages: road.messages,
                    lastFrom,
                    level: level(road, home.now(), limits.road.growsOverDays),
                });
            }
            hamlets.push({
                id: hamlet,
                name: place.title ?? hamlet,
                at: place.at,
                roads,
                votes: await voteViews(home, hamlet, context),
                folder: work.folder,
                before: [...work.before],
                mode: work.mode,
                modeWords: modeWords(work.mode),
                limits: structuredClone(limits),
                budget: await reportOf(home, hamlet, context, undefined, books),
            });
            const departed = record.archive.filter(
                (a) =>
                    a.back === null &&
                    !record.agents.some(
                        (live) => live.number === a.agent.number,
                    ),
            );
            for (const agent of [
                ...record.agents,
                ...departed.map((a) => a.agent),
            ]) {
                const ref = { hamlet, number: agent.number },
                    key = agentKey(ref);
                const work = await chatState(home, ref, activity, context);
                currentChats.set(key, work.currents);
                const active = work.active;
                const spent = books.agents[String(agent.number)] ?? {
                    tokens: 0,
                    micros: 0,
                    ms: 0,
                };
                const left = departed.find(
                    (a) => a.agent.number === agent.number,
                );
                const busy = !left && work.busy;
                agents.push({
                    ref,
                    name: agent.name,
                    purpose: agent.purpose,
                    since: agent.since,
                    mayor: !left && record.mayor === agent.number,
                    model: choose(limits, settings, {
                        name: agent.name,
                        mayor: !left && record.mayor === agent.number,
                        model: agent.model,
                    }),
                    busy,
                    faulted: !busy && work.faulted,
                    activity: structuredClone(
                        left
                            ? activityOf()
                            : (active?.activity ?? activityOf()),
                    ),
                    saying: structuredClone(
                        active?.saying ?? { text: "", thought: false },
                    ),
                    spent: { ...spent, cost: money(spent.micros) },
                    skills: left
                        ? []
                        : await home.houses.of(ref).skillNames(context),
                    consulting: !left && work.consulting,
                    conversations: await conversationViews(home, ref, context),
                    ...(left
                        ? {
                              left: {
                                  how: left.how,
                                  into:
                                      left.into === null
                                          ? null
                                          : { hamlet, number: left.into },
                                  at: left.at,
                                  by: byWords(left.by),
                                  reason: left.reason,
                                  words: farewell(left, record),
                              },
                          }
                        : {}),
                });
            }
            for (const permission of ["network", "unconfined"] as const)
                for (const name of limits.permissions[permission]) {
                    const agent = named(record, name);
                    grants.push({
                        agent: agent ? { hamlet, number: agent.number } : null,
                        name,
                        permission,
                        by: null,
                        source: "limits",
                    });
                }
        }
        return {
            hamlets,
            agents,
            grants,
            records,
            currentChats,
            closed: (realm?.hamlets ?? []).flatMap((h) =>
                h.removed && !h.erased
                    ? [
                          {
                              id: h.id,
                              name: h.title ?? h.id,
                              at: h.removed.at,
                              by: byWords(h.removed.by),
                              reason: h.removed.reason,
                          },
                      ]
                    : [],
            ),
        };
    },
};
function byWords(by: string): string {
    return by === "mayor"
        ? "the mayor"
        : by.startsWith("chief:")
          ? `the chief ${by.slice(6)}`
          : by;
}
