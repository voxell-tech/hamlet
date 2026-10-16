import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import {
    type AgentEventStream,
    type ConversationId,
    InboxDoc,
    LiveDoc,
    watchEvents,
} from "@earendil-works/pi-durable";
import { Activity } from "./activity.ts";
import { conversationViews } from "./agent-chats.ts";
import { ChatDoc, currentOf } from "./chats.ts";
import { choose } from "./choice.ts";
import { DEEDS } from "./commands.ts";
import { GrantsDoc, grantsOf } from "./grants.ts";
import { agentKey, WorkDoc } from "./hamlet.ts";
import type { Home } from "./home.ts";
import { houseOf } from "./layout.ts";
import { booksOf } from "./ledger.ts";
import { STARTERS } from "./library.ts";
import { LimitsDoc, limitsOf } from "./limits.ts";
import { providersOf } from "./logins.ts";
import { money } from "./money.ts";
import type {
    Agent,
    Changes,
    Grant,
    Hamlet,
    Road,
    State,
    Update,
    Wait,
} from "./protocol.ts";
import {
    farewell,
    level,
    liveHamlets,
    RealmDoc,
    realmRoads,
    roadsOf,
} from "./realm.ts";
import { addressOf, type HamletRecord, named, RecordDoc } from "./record.ts";
import { reportOf } from "./report.ts";
import { type Settings, SettingsDoc, settingsOf } from "./settings.ts";
import { refreshSkills, skillNames, starterSkills } from "./skills.ts";
import { AgentConversationsDoc } from "./society/docs.ts";
import { libraryView } from "./society/librarian.ts";
import { voteViews } from "./society/votes.ts";
import { OversightDoc } from "./waits.ts";
import { queue, says } from "./words.ts";
import { modeWords } from "./work.ts";

const starters = starterSkills(STARTERS);

type Work = Pick<
    Activity,
    "activity" | "saying" | "working" | "pending" | "faulted" | "consulting"
>;

/** The whole realm's state, which a client starts from (S5). */
export async function stateOf(
    home: Home,
    at: { epoch: number; revision: number },
    context: Context,
    activity: ReadonlyMap<ConversationId, Work> = new Map(),
    savedSettings?: Settings,
): Promise<State> {
    const read = home.harness;
    const realm = await read.snapshot(RealmDoc, context);
    const places = liveHamlets(realm);
    const records: Record<string, Readonly<HamletRecord>> = {};
    for (const h of places)
        records[h.id] =
            (await read.snapshot(RecordDoc, h.id, context)) ??
            RecordDoc.definition.initial(h.id);
    const settings = settingsOf(
        savedSettings ?? (await read.snapshot(SettingsDoc, context)),
        home.overrides,
    );
    const oversight = await read.snapshot(OversightDoc, context);
    const hamlets: Hamlet[] = [],
        agents: Agent[] = [],
        grants: Grant[] = grantsOf(await read.snapshot(GrantsDoc, context)).map(
            (g) => ({ ...g, source: "grant" }),
        );
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
                !record.agents.some((live) => live.number === a.agent.number),
        );
        for (const agent of [
            ...record.agents,
            ...departed.map((a) => a.agent),
        ]) {
            const ref = { hamlet, number: agent.number },
                key = agentKey(ref);
            const chats = await read.snapshot(ChatDoc, key, context);
            currentChats.set(
                key,
                Object.fromEntries(
                    Object.entries(chats?.chats ?? {}).map(([chief, chat]) => [
                        chief,
                        chat.current,
                    ]),
                ),
            );
            const index = await read.snapshot(
                AgentConversationsDoc,
                key,
                context,
            );
            const works: Work[] = [];
            for (const id of index?.ids ?? []) {
                const observed = activity.get(id);
                if (observed) works.push(observed);
                else {
                    const live = await read.snapshot(LiveDoc, id, context);
                    const inbox = await read.snapshot(InboxDoc, id, context);
                    works.push({
                        working: live?.run !== undefined,
                        pending:
                            inbox?.items.filter((i) => i.mode !== "write")
                                .length ?? 0,
                        faulted: false,
                        consulting: false,
                        activity: {
                            doing: live?.run === undefined ? "rest" : "think",
                            doingWords:
                                live?.run === undefined
                                    ? "At rest"
                                    : "Thinking",
                            step: 0,
                            failures: 0,
                        },
                        saying: { text: "", thought: false },
                    });
                }
            }
            const current = chats?.chats[home.chief];
            const active =
                works.find((w) => w.working) ??
                (current && activity.get(currentOf(current))) ??
                works.at(-1);
            const spent = books.agents[String(agent.number)] ?? {
                tokens: 0,
                micros: 0,
                ms: 0,
            };
            const left = departed.find((a) => a.agent.number === agent.number);
            const busy = !left && works.some((w) => w.working || w.pending > 0);
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
                faulted:
                    !busy &&
                    (current
                        ? (activity.get(currentOf(current))?.faulted ?? false)
                        : false),
                activity: structuredClone(
                    left
                        ? {
                              doing: "rest",
                              doingWords: "At rest",
                              step: 0,
                              failures: 0,
                          }
                        : (active?.activity ?? {
                              doing: "rest",
                              doingWords: "At rest",
                              step: 0,
                              failures: 0,
                          }),
                ),
                saying: structuredClone(
                    active?.saying ?? { text: "", thought: false },
                ),
                spent: { ...spent, cost: money(spent.micros) },
                skills: left
                    ? []
                    : await skillNames(
                          join(houseOf(home.layout, ref), "skills"),
                      ),
                consulting: !left && works.some((w) => w.consulting),
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
    const waits = queue(oversight).map((wait): Wait => {
        const record = wait.chat && records[wait.chat.hamlet];
        return {
            id: wait.id,
            ...(wait.kind === "approval"
                ? {
                      kind: "approval" as const,
                      danger: wait.danger ?? "unconfined",
                  }
                : { kind: wait.kind, danger: null }),
            by: wait.by,
            agent: wait.agent && { ...wait.agent },
            says: says(wait.danger),
            about: wait.about,
            change: wait.change && structuredClone(wait.change),
            answers: [...wait.answers],
            since: wait.since,
            asked: wait.asked,
            chat:
                wait.chat && record
                    ? {
                          with: {
                              hamlet: wait.chat.hamlet,
                              agent: addressOf(record, wait.chat.number),
                          },
                          ...(currentChats.get(agentKey(wait.chat))?.[
                              wait.chat.chief ?? ""
                          ] === wait.chat.chat
                              ? {}
                              : { past: wait.chat.chat }),
                      }
                    : null,
        };
    });
    return {
        type: "state",
        houses: home.layout.houses,
        ...at,
        chief: home.chief,
        sandbox: home.sandbox.name,
        deeds: [...DEEDS],
        starters: structuredClone(await starters),
        hamlets,
        agents,
        waits,
        grants,
        library: await libraryView(home, context),
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
        settings: {
            model: settings.model,
            reflectEvery: settings.reflectEvery,
            allowNet: settings.allowNet,
            endpoints: await Promise.all(
                settings.endpoints.map(async (endpoint) => ({
                    ...endpoint,
                    key:
                        (await home.credentials.read(endpoint.name)) !==
                        undefined,
                })),
            ),
            unset: !settings.saved && home.overrides.model === undefined,
            link: settings.link,
        },
        providers: await providersOf(home),
    };
}
function byWords(by: string): string {
    return by === "mayor"
        ? "the mayor"
        : by.startsWith("chief:")
          ? `the chief ${by.slice(6)}`
          : by;
}

/** The realm's current snapshot and serialized changes, including unopened chats. */
export async function watchState(
    home: Home,
    send: (message: Changes | State) => void,
    context: Context,
) {
    const at = { epoch: home.now(), revision: 0 };
    const activity = new Map<ConversationId, Activity>();
    const watches = new Map<ConversationId, AgentEventStream>();
    const endings = new Map<string, string>();
    let savedSettings = await home.harness.snapshot(SettingsDoc, context);
    let stopped = false;
    let state = await stateOf(home, at, context);
    let line = Promise.resolve();
    const discover = async () => {
        const wanted = new Set<ConversationId>();
        const realm = await home.harness.snapshot(RealmDoc, context);
        for (const { id: hamlet } of liveHamlets(realm)) {
            const record = await home.harness.snapshot(
                RecordDoc,
                hamlet,
                context,
            );
            for (const agent of [
                ...(record?.agents ?? []),
                ...(record?.archive
                    .filter((a) => a.back === null)
                    .map((a) => a.agent) ?? []),
            ]) {
                const index = await home.harness.snapshot(
                    AgentConversationsDoc,
                    agentKey({ hamlet, number: agent.number }),
                    context,
                );
                for (const id of index?.ids ?? []) {
                    wanted.add(id);
                    if (watches.has(id)) continue;
                    const watch = await watchEvents(home.harness, id, context);
                    watches.set(id, watch);
                    const work = new Activity(watch.snapshot);
                    activity.set(id, work);
                    watch.start(async (events) => {
                        for (const event of events) work.note(event);
                        if (!work.working) refreshSkills(home.layout.houses);
                        schedule();
                    });
                }
            }
        }
        for (const [id, watch] of watches)
            if (!wanted.has(id)) {
                await watch.stop();
                watches.delete(id);
                activity.delete(id);
            }
    };
    const schedule = () => {
        if (stopped) return line;
        const works = new Map(
            [...activity].map(([id, work]) => [id, structuredClone(work)]),
        );
        const settings = structuredClone(savedSettings);
        line = line
            .then(async () => {
                if (stopped) return;
                await discover();
                for (const [id, work] of activity)
                    if (!works.has(id)) works.set(id, structuredClone(work));
                const next = await stateOf(home, at, context, works, settings);
                const changes = changed(state, next, endings);
                const whole =
                    JSON.stringify(state.library) !==
                        JSON.stringify(next.library) ||
                    JSON.stringify(state.closed) !==
                        JSON.stringify(next.closed) ||
                    state.hamlets.some(
                        (h) => !next.hamlets.some((n) => n.id === h.id),
                    );
                if (!whole && changes.length === 0) return;
                at.revision++;
                state = { ...next, ...at };
                send(
                    whole
                        ? structuredClone(state)
                        : { type: "change", ...at, changes },
                );
            })
            .catch((error: unknown) => {
                if (!stopped)
                    process.stderr.write(
                        `hamlet: could not show the realm: ${String(error)}\n`,
                    );
            });
        return line;
    };
    const unsubscribe = home.harness.subscribeCommits(({ changes }) => {
        refreshSkills(home.layout.houses);
        for (const change of changes) {
            if (change.type !== "document" || change.value === null) continue;
            if (change.record.kind === "hamlet.settings")
                savedSettings = change.value as Settings;
            if (
                change.record.kind === "hamlet.oversight-day" &&
                Array.isArray(change.value.events)
            )
                for (const event of change.value.events) {
                    if (
                        event !== null &&
                        typeof event === "object" &&
                        !Array.isArray(event) &&
                        event.type === "settled" &&
                        typeof event.id === "string" &&
                        typeof event.how === "string"
                    )
                        endings.set(event.id, event.how);
                }
        }
        if (
            changes.some(
                (change) =>
                    change.type === "document" &&
                    [
                        "hamlet.chat",
                        "hamlet.record",
                        "hamlet.work",
                        "hamlet.limits",
                        "hamlet.settings",
                        "hamlet.oversight",
                        "hamlet.grants",
                        "hamlet.ledger",
                        "hamlet.realm",
                        "hamlet.votes",
                        "hamlet.library",
                        "hamlet.instructions",
                        "hamlet.agent-conversations",
                        "hamlet.peers",
                    ].includes(change.record.kind),
            )
        )
            void schedule();
    });
    await discover();
    await schedule();
    return {
        snapshot: async () => {
            refreshSkills(home.layout.houses);
            await schedule();
            return structuredClone(state);
        },
        refresh: schedule,
        stop: async () => {
            stopped = true;
            unsubscribe();
            refreshSkills(home.layout.houses);
            await Promise.all(
                [...watches.values()].map((watch) => watch.stop()),
            );
            await line;
        },
    };
}

function changed(
    before: State,
    after: State,
    endings: ReadonlyMap<string, string>,
): Update[] {
    const changes: Update[] = [];
    const differs = (a: unknown, b: unknown) =>
        JSON.stringify(a) !== JSON.stringify(b);
    for (const hamlet of after.hamlets)
        if (
            differs(
                before.hamlets.find((each) => each.id === hamlet.id),
                hamlet,
            )
        )
            changes.push({ type: "hamlet", hamlet });
    for (const agent of after.agents)
        if (
            differs(
                before.agents.find(
                    (each) => agentKey(each.ref) === agentKey(agent.ref),
                ),
                agent,
            )
        )
            changes.push({ type: "agent", agent });
    for (const agent of before.agents)
        if (
            !after.agents.some(
                (each) => agentKey(each.ref) === agentKey(agent.ref),
            )
        )
            changes.push({ type: "agent-gone", ref: agent.ref });
    for (const wait of after.waits)
        if (
            differs(
                before.waits.find((each) => each.id === wait.id),
                wait,
            )
        )
            changes.push({ type: "wait", wait });
    for (const wait of before.waits)
        if (!after.waits.some((each) => each.id === wait.id))
            changes.push({
                type: "wait-closed",
                id: wait.id,
                how: endings.get(wait.id) ?? "lapsed",
            });
    if (differs(before.grants, after.grants))
        changes.push({ type: "grants", grants: after.grants });
    if (differs(before.settings, after.settings))
        changes.push({ type: "settings", settings: after.settings });
    if (differs(before.providers, after.providers))
        changes.push({ type: "providers", providers: after.providers });
    return changes;
}
