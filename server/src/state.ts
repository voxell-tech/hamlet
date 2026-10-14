import type { Context } from "@earendil-works/chord";
import {
    type AgentEventStream,
    type ConversationId,
    InboxDoc,
    LiveDoc,
    watchEvents,
} from "@earendil-works/pi-durable";
import { Activity } from "./activity.ts";
import { ChatDoc, conversationsOf, currentOf } from "./chats.ts";
import { choose } from "./choice.ts";
import { GrantsDoc, grantsOf } from "./grants.ts";
import { agentKey, HOME, WorkDoc } from "./hamlet.ts";
import type { Home } from "./home.ts";
import { booksOf } from "./ledger.ts";
import { LimitsDoc, limitsOf } from "./limits.ts";
import { providersOf } from "./logins.ts";
import { money } from "./money.ts";
import type {
    Agent,
    Changes,
    Grant,
    Hamlet,
    State,
    Update,
    Wait,
} from "./protocol.ts";
import { addressOf, named, RecordDoc } from "./record.ts";
import { reportOf } from "./report.ts";
import { type Settings, SettingsDoc, settingsOf } from "./settings.ts";
import { OversightDoc } from "./waits.ts";
import { queue, says } from "./words.ts";
import { modeWords } from "./work.ts";

type Work = Pick<
    Activity,
    "activity" | "saying" | "working" | "pending" | "faulted"
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
    const record =
        (await read.snapshot(RecordDoc, HOME, context)) ??
        RecordDoc.definition.initial(HOME);
    const limits = limitsOf(await read.snapshot(LimitsDoc, HOME, context));
    const work =
        (await read.snapshot(WorkDoc, HOME, context)) ??
        WorkDoc.definition.initial(HOME);
    const settings = settingsOf(
        savedSettings ?? (await read.snapshot(SettingsDoc, context)),
        home.overrides,
    );
    const oversight = await read.snapshot(OversightDoc, context);
    const books = await booksOf(
        home,
        HOME,
        limits.budget.period,
        null,
        context,
    );
    const budget = await reportOf(home, HOME, context, undefined, books);
    const hamlet: Hamlet = {
        id: HOME,
        name: HOME,
        folder: work.folder,
        before: [...work.before],
        mode: work.mode,
        modeWords: modeWords(work.mode),
        limits: structuredClone(limits),
        budget,
    };
    const agents: Agent[] = [];
    const currentChats = new Map<number, Record<string, number>>();
    for (const agent of record.agents) {
        const ref = { hamlet: HOME, number: agent.number };
        const chats = await read.snapshot(ChatDoc, agentKey(ref), context);
        currentChats.set(
            agent.number,
            Object.fromEntries(
                Object.entries(chats?.chats ?? {}).map(([chief, chat]) => [
                    chief,
                    chat.current,
                ]),
            ),
        );
        const works: Work[] = [];
        for (const id of chats ? conversationsOf(chats) : []) {
            const observed = activity.get(id);
            if (observed !== undefined) works.push(observed);
            else {
                const live = await read.snapshot(LiveDoc, id, context);
                const inbox = await read.snapshot(InboxDoc, id, context);
                works.push({
                    working: live?.run !== undefined,
                    pending: inbox?.items.length ?? 0,
                    faulted: false,
                    activity: {
                        doing: live?.run === undefined ? "rest" : "think",
                        doingWords:
                            live?.run === undefined ? "At rest" : "Thinking",
                        step: 0,
                        failures: 0,
                    },
                    saying: { text: "", thought: false },
                });
            }
        }
        const current = chats?.chats[home.chief];
        const active =
            works.find((work) => work.working) ??
            (current && activity.get(currentOf(current))) ??
            works.at(-1);
        const spent = books.agents[String(agent.number)] ?? {
            tokens: 0,
            micros: 0,
            ms: 0,
        };
        const busy = works.some((work) => work.working || work.pending > 0);
        agents.push({
            ref,
            name: agent.name,
            purpose: agent.purpose,
            since: agent.since,
            mayor: record.mayor === agent.number,
            model: choose(limits, settings, {
                name: agent.name,
                mayor: record.mayor === agent.number,
            }),
            busy,
            faulted: !busy && (active?.faulted ?? false),
            activity: structuredClone(
                active?.activity ?? {
                    doing: "rest",
                    doingWords: "At rest",
                    step: 0,
                    failures: 0,
                },
            ),
            saying: structuredClone(
                active?.saying ?? { text: "", thought: false },
            ),
            spent: { ...spent, cost: money(spent.micros) },
        });
    }
    const waits = queue(oversight).map(
        (wait): Wait => ({
            id: wait.id,
            kind: wait.kind,
            danger: wait.danger,
            by: wait.by,
            agent: wait.agent && { ...wait.agent },
            says: says(wait.danger),
            about: wait.about,
            change: wait.change && structuredClone(wait.change),
            answers: [...wait.answers],
            since: wait.since,
            asked: wait.asked,
            chat: wait.chat && {
                with: {
                    hamlet: wait.chat.hamlet,
                    agent: addressOf(record, wait.chat.number),
                },
                ...(currentChats.get(wait.chat.number)?.[
                    wait.chat.chief ?? ""
                ] === wait.chat.chat
                    ? {}
                    : { past: wait.chat.chat }),
            },
        }),
    );
    const grants: Grant[] = grantsOf(
        await read.snapshot(GrantsDoc, context),
    ).map((grant) => ({ ...grant, source: "grant" }));
    for (const permission of ["network", "unconfined"] as const)
        for (const name of limits.permissions[permission]) {
            const agent = named(record, name);
            grants.push({
                agent:
                    agent === undefined
                        ? null
                        : { hamlet: HOME, number: agent.number },
                name,
                permission,
                by: null,
                source: "limits",
            });
        }
    return {
        type: "state",
        ...at,
        chief: home.chief,
        sandbox: home.sandbox.name,
        hamlets: [hamlet],
        agents,
        waits,
        grants,
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

/** The realm's current snapshot and serialized changes, including unopened chats. */
export async function watchState(
    home: Home,
    send: (message: Changes) => void,
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
        const record = await home.harness.snapshot(RecordDoc, HOME, context);
        for (const agent of record?.agents ?? []) {
            const chats = await home.harness.snapshot(
                ChatDoc,
                agentKey({ hamlet: HOME, number: agent.number }),
                context,
            );
            for (const id of chats ? conversationsOf(chats) : []) {
                if (watches.has(id)) continue;
                const watch = await watchEvents(home.harness, id, context);
                watches.set(id, watch);
                const work = new Activity(watch.snapshot);
                activity.set(id, work);
                watch.start(async (events) => {
                    for (const event of events) work.note(event);
                    schedule();
                });
            }
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
                if (changes.length === 0) return;
                at.revision++;
                state = { ...next, ...at };
                send({ type: "change", ...at, changes });
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
                    ].includes(change.record.kind),
            )
        )
            void schedule();
    });
    await discover();
    await schedule();
    return {
        snapshot: async () => {
            await schedule();
            return structuredClone(state);
        },
        refresh: schedule,
        stop: async () => {
            stopped = true;
            unsubscribe();
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
