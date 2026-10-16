import type { Context } from "@earendil-works/chord";
import {
    type AgentEventStream,
    type ConversationId,
    watchEvents,
} from "@earendil-works/pi-durable";
import { conversationIds } from "./chats.ts";
import { DEEDS } from "./commands.ts";
import type { Home } from "./home.ts";
import { refreshSkills, starterSkills } from "./house/skills.ts";
import { LibraryDoc, STARTERS } from "./library/shelf.ts";
import { libraryView } from "./library.ts";
import { modelView } from "./models.ts";
import { ended, waitsView } from "./oversight.ts";
import type { Changes, State, Update } from "./protocol.ts";
import { realmView } from "./realm.ts";
import { agentKey } from "./record.ts";
import { Activity, type Work } from "./state/activity.ts";

/** Owner-provided state values and the document definitions that invalidate them. */
export type View<T, Folded = void> = {
    docs: readonly Pick<
        import("@earendil-works/pi-durable").DocDefinition<
            import("@earendil-works/pi-durable").JsonObject
        >,
        "kind"
    >[];
    build(home: Home, context: Context, folded: Folded): Promise<T>;
};

const starters = starterSkills(STARTERS);
const libraryState: View<NonNullable<State["library"]>> = {
    docs: [LibraryDoc.definition],
    build: (home, context) => libraryView(home, context),
};
const triggerKinds: ReadonlySet<string> = new Set(
    [
        ...realmView.docs,
        ...modelView.docs,
        ...waitsView.docs,
        ...libraryState.docs,
    ].map((doc) => doc.kind),
);

/** The whole realm's state, composed from its owners' views (S5). */
export async function stateOf(
    home: Home,
    at: { epoch: number; revision: number },
    context: Context,
    activity: ReadonlyMap<ConversationId, Work> = new Map(),
): Promise<State> {
    const models = await modelView.build(home, context, undefined);
    const realm = await realmView.build(home, context, {
        activity,
        settings: models.saved,
    });
    const waits = await waitsView.build(home, context, realm);
    return {
        type: "state",
        houses: home.layout.houses,
        ...at,
        chief: home.chief,
        sandbox: home.sandbox.name,
        deeds: [...DEEDS],
        starters: structuredClone(await starters),
        hamlets: realm.hamlets,
        agents: realm.agents,
        grants: realm.grants,
        closed: realm.closed,
        waits,
        settings: models.settings,
        providers: models.providers,
        library: await libraryState.build(home, context, undefined),
    };
}

/** The realm's current snapshot and serialized changes, including unopened chats. */
export async function watchState(
    home: Home,
    send: (message: Changes | State) => void,
    context: Context,
    report: (error: unknown) => void = console.error,
) {
    const at = { epoch: home.now(), revision: 0 };
    const activity = new Map<ConversationId, Activity>();
    const watches = new Map<ConversationId, AgentEventStream>();
    let stopped = false;
    let state = await stateOf(home, at, context);
    let line = Promise.resolve();
    const discover = async () => {
        const wanted = new Set<ConversationId>();
        for (const id of await conversationIds(home, context)) {
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
        line = line
            .then(async () => {
                if (stopped) return;
                await discover();
                for (const [id, work] of activity)
                    if (!works.has(id)) works.set(id, structuredClone(work));
                const next = await stateOf(home, at, context, works);
                const endings = await ended(
                    home.harness,
                    state.waits
                        .filter(
                            (wait) =>
                                !next.waits.some((each) => each.id === wait.id),
                        )
                        .map((wait) => wait.id),
                    context,
                    home.now(),
                );
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
                if (!stopped) report(error);
            });
        return line;
    };
    const unsubscribe = home.harness.subscribeCommits(({ changes }) => {
        refreshSkills(home.layout.houses);
        if (
            changes.some(
                (change) =>
                    change.type === "document" &&
                    triggerKinds.has(change.record.kind),
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
