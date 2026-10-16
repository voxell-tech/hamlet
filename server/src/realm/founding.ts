import { lstat } from "node:fs/promises";
import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import { Type } from "@earendil-works/pi-ai";
import {
    type DocumentReader,
    defineExtension,
    defineTool,
    section,
    type Tx,
} from "@earendil-works/pi-durable";
import {
    agentOf,
    ChatDoc,
    currentOf,
    notify,
    startConversation,
    stopAgent,
    WithDoc,
} from "../chats.ts";
import { WorkDoc } from "../environment.ts";
import { GrantsDoc, grantsOf, revoke } from "../gate.ts";
import type { Deed, Home, Part } from "../home.ts";
import { buildHouse } from "../home.ts";
import { removeHouse } from "../house.ts";
import { type Limits, LimitsDoc, limitsOf } from "../limits.ts";
import { reconfigure } from "../models.ts";
import { lapseAgent } from "../oversight.ts";
import {
    agentKey,
    checkName,
    defineRecordTool,
    HOME,
    RecordDoc,
} from "../record.ts";
import { awaiting } from "./dealings.ts";
import { chiefsOf, liveHamlets, type Realm, RealmDoc } from "./places.ts";
import { realmRoads } from "./roads.ts";

/** The realm's founding and closing tools and prompt. */
export const Founding: Part = (home) => {
    let stop: (() => Promise<unknown>) | undefined;
    return {
        extension: defineExtension({
            name: "hamlet.founding",
            tools: [
                defineTool({
                    name: "ask_removal",
                    description:
                        "Ask the chief to remove a hamlet. Nothing is removed until a chief confirms.",
                    parameters: Type.Object({
                        hamlet: Type.String(),
                        reason: Type.String(),
                    }),
                    replay: "safe",
                    execute: async (args, api, context) =>
                        defineRecordTool({
                            name: "ask_removal",
                            description: "Ask for removal",
                            parameters: Type.Object({
                                hamlet: Type.String(),
                                reason: Type.String(),
                            }),
                            change: async (tx, args) => {
                                const caller = await tx.doc(
                                    WithDoc,
                                    api.conversationId,
                                );
                                const realm = await tx.doc(RealmDoc);
                                const live = liveHamlets(realm);
                                const own = await tx.doc(
                                    RecordDoc,
                                    caller.hamlet,
                                    caller.hamlet,
                                );
                                if (
                                    own.mayor !== caller.number ||
                                    !live.some((h) => h.id === caller.hamlet) ||
                                    live.length < 2
                                )
                                    return "Only a mayor, while another hamlet lives, can ask for a removal.";
                                const target = live.find(
                                    (h) => h.id === args.hamlet,
                                );
                                if (!target)
                                    return `no hamlet named \`${args.hamlet}\``;
                                if (!args.reason.trim())
                                    return "say why in `reason`; the chief is shown it";
                                realm.removals.push({
                                    hamlet: args.hamlet,
                                    by: caller.hamlet,
                                    reason: args.reason.trim(),
                                    at: home.now(),
                                });
                                const asked = new Set(
                                    realm.removals
                                        .filter(
                                            (r) =>
                                                r.hamlet === target.id &&
                                                live.some((h) => h.id === r.by),
                                        )
                                        .map((r) => r.by),
                                );
                                const limits = await tx.doc(
                                    LimitsDoc,
                                    HOME,
                                    HOME,
                                );
                                const ready =
                                    asked.has(target.id) ||
                                    [...asked].filter((id) => id !== target.id)
                                        .length >= limits.hamlets.removalMayors;
                                const words = `The mayor of \`${caller.hamlet}\` asks for the hamlet \`${target.id}\` to be removed. ${ready ? `Enough have asked: /remove-hamlet ${target.id} confirms it.` : "Not enough mayors have asked yet."}`;
                                await tellChiefs(home, tx, target.id, words);
                                return words;
                            },
                        }).execute(args, api, context),
                }),
            ],
            sections: [
                section(
                    "realm",
                    async (input, context) => {
                        const agent = await agentOf(
                            input.read,
                            input.conversationId,
                            context,
                        );
                        if (!agent) return undefined;
                        const realm = await input.read.snapshot(
                            RealmDoc,
                            context,
                        );
                        if (!realm || liveHamlets(realm).length < 2)
                            return undefined;
                        const records = Object.fromEntries(
                            await Promise.all(
                                liveHamlets(realm).map(
                                    async ({ id }) =>
                                        [
                                            id,
                                            await input.read.snapshot(
                                                RecordDoc,
                                                id,
                                                context,
                                            ),
                                        ] as const,
                                ),
                            ),
                        );
                        const own = records[agent.hamlet];
                        if (!own) return undefined;
                        if (own.mayor !== agent.number) {
                            const peers = realmRoads(
                                realm,
                                Object.fromEntries(
                                    Object.entries(records).flatMap(
                                        ([id, record]) =>
                                            record ? [[id, record]] : [],
                                    ),
                                ),
                            ).flatMap((road) => {
                                const other =
                                    road.a.hamlet === agent.hamlet &&
                                    road.a.number === agent.number
                                        ? road.b
                                        : road.b.hamlet === agent.hamlet &&
                                            road.b.number === agent.number
                                          ? road.a
                                          : undefined;
                                if (!other) return [];
                                const record = records[other.hamlet];
                                return [
                                    other.number === record?.mayor
                                        ? `mayor@${other.hamlet}`
                                        : (record?.agents.find(
                                              (a) => a.number === other.number,
                                          )?.name ?? ""),
                                ];
                            });
                            return peers.length
                                ? `Your roads to agents of other hamlets lead to: ${peers.join(", ")}. Whoever asks pays for the answer.`
                                : undefined;
                        }
                        const lines = [
                            "# Other hamlets",
                            "You have a road to every other mayor: reach one with `ask_agent` as `mayor@<hamlet>`. Whoever asks pays for the answer. Between two hamlets a thing is done only when both mayors agree: `agree_road` for a road between one of your agents and one of theirs, `agree_move` for an agent to move with its house. `ask_removal` asks the chief to remove a hamlet.",
                        ];
                        for (const { id } of liveHamlets(realm)) {
                            if (id === agent.hamlet) continue;
                            const record = records[id];
                            if (!record) continue;
                            lines.push(
                                `- ${id}: mayor \`${record.agents.find((a) => a.number === record.mayor)?.name}\`; agents: ${
                                    record.agents
                                        .filter(
                                            (a) => a.number !== record.mayor,
                                        )
                                        .map((a) => a.name)
                                        .join(", ") || "(none)"
                                }`,
                            );
                        }
                        const waiting = await awaiting(
                            home,
                            agent.hamlet,
                            context,
                        );
                        if (waiting)
                            lines.push("Waiting for your decision:", waiting);
                        return lines.join("\n");
                    },
                    { tag: false },
                ),
            ],
        }),
        async start(context) {
            await finish(home, context);
            const watch = await home.harness.watchDoc(RealmDoc, context);
            if (watch) {
                watch.start((_value, _ops, context) => finish(home, context));
                stop = () => watch.stop();
            }
        },
        async stop() {
            await stop?.();
        },
    };
};

/** Founds a hamlet for the chief and builds its mayor's house. */
export const found: Deed = async (home, given, context) => {
    const [hamlet = "", mayor = "", ...purpose] = given.rest
        .trim()
        .split(/\s+/);
    await home.harness.commit(
        (tx) =>
            foundHamlet(home, tx, {
                hamlet,
                mayor,
                purpose: purpose.join(" "),
                chiefs: [given.chief],
                from: null,
            }),
        context,
    );
    await buildHouse(home.layout, { hamlet, number: 0 });
    return `The hamlet \`${hamlet}\` is founded under the mayor \`${mayor}\`, and it is yours. /hamlet ${hamlet} talks to it.`;
};
/** Lists the realm's living hamlets by title and id. */
export const hamlets: Deed = async (home, _given, context) => {
    const realm = await home.harness.snapshot(RealmDoc, context);
    return liveHamlets(realm)
        .map((h) => `- ${h.title ?? h.id} (\`${h.id}\`)`)
        .join("\n");
};
/** Sets or clears a chief-controlled hamlet's displayed title. */
export const nameHamlet: Deed = async (home, given, context) => {
    const [id = "", ...words] = given.rest.trim().split(/\s+/);
    const title = words.join(" ");
    if ([...title].length > 32)
        throw new Error("a hamlet's name is at most 32 letters");
    await home.harness.commit(async (tx) => {
        (await controlled(tx, id, given.chief, "name")).title = title || null;
    }, context);
    return `The hamlet \`${id}\` is shown as ${title || id}.`;
};
/** Sets a chief-controlled hamlet's position in the world. */
export const moveHamlet: Deed = async (home, given, context) => {
    const [id = "", x, z, extra] = given.rest.trim().split(/\s+/);
    if (
        x === undefined ||
        z === undefined ||
        extra !== undefined ||
        !Number.isFinite(Number(x)) ||
        !Number.isFinite(Number(z))
    )
        throw new Error("a place in the world is two numbers");
    await home.harness.commit(async (tx) => {
        (await controlled(tx, id, given.chief, "move")).at = {
            x: Number(x),
            z: Number(z),
        };
    }, context);
    return `The town of \`${id}\` is set down at ${x}, ${z}.`;
};
/** Archives a chief-controlled hamlet and finishes its pending house work. */
export const removeHamletDeed: Deed = async (home, given, context) => {
    const [hamlet = "", ...reason] = given.rest.trim().split(/\s+/);
    await home.harness.commit(async (tx) => {
        await controlled(tx, hamlet, given.chief, "remove");
        await removeHamlet(home, tx, {
            hamlet,
            by: given.chief,
            reason: reason.join(" "),
        });
    }, context);
    await finish(home, context);
    return `The hamlet \`${hamlet}\` was removed and its houses archived.`;
};
/** Restores a chief-controlled hamlet and its recorded agents and roads. */
export const restoreHamlet: Deed = async (home, given, context) => {
    const id = given.rest.trim();
    await home.harness.commit(async (tx) => {
        const place = await controlled(tx, id, given.chief, "restore", true);
        const realm = await tx.doc(RealmDoc);
        const live = liveHamlets(realm);
        if (live.length >= (await tx.doc(LimitsDoc, HOME, HOME)).hamlets.most)
            throw new Error("the chief's limits allow no more hamlets");
        const record = await tx.doc(RecordDoc, id, id);
        for (const other of live) {
            const names = (
                await tx.doc(RecordDoc, other.id, other.id)
            ).agents.map((a) => a.name);
            const taken = record.agents.find((a) => names.includes(a.name));
            if (taken)
                throw new Error(
                    `an agent named \`${taken.name}\` exists now, so \`${id}\` cannot come back as it was`,
                );
        }
        place.removed = null;
        realm.removals = realm.removals.filter((r) => r.hamlet !== id);
        await reconfigure(home, tx);
    }, context);
    await finish(home, context);
    return `The hamlet \`${id}\` is restored.`;
};
/** Marks a removed hamlet erased and sweeps its houses for deletion. */
export const eraseHamlet: Deed = async (home, given, context) => {
    const id = given.rest.trim();
    await home.harness.commit(async (tx) => {
        const place = await controlled(tx, id, given.chief, "delete", true);
        refusePendingMove(await tx.doc(RealmDoc), id);
        place.erased = true;
    }, context);
    await finish(home, context);
    return `The hamlet \`${id}\` is deleted for good.`;
};
async function controlled(
    tx: Tx,
    id: string,
    chief: string,
    verb: string,
    removed = false,
) {
    const place = (await tx.doc(RealmDoc)).hamlets.find(
        (h) =>
            h.id === id &&
            !h.erased &&
            (removed ? h.removed !== null : h.removed === null),
    );
    if (!place)
        throw new Error(
            `no ${removed ? "removed " : ""}hamlet named \`${id}\``,
        );
    if (place.chiefs.length > 0 && !place.chiefs.includes(chief))
        throw new Error(`only a chief of \`${id}\` can ${verb} it`);
    return place;
}
async function tellChiefs(
    home: Home,
    tx: Tx,
    hamlet: string,
    words: string,
): Promise<void> {
    const realm = await tx.doc(RealmDoc);
    const record = await tx.doc(RecordDoc, hamlet, hamlet);
    const key = agentKey({ hamlet, number: record.mayor });
    for (const chief of chiefsOf(realm, hamlet, home.chief)) {
        const chats = (await tx.doc(ChatDoc, key, key)).chats;
        let chat = chats[chief];
        if (!chat) {
            const conversation = await startConversation(home, tx, {
                hamlet,
                number: record.mayor,
                chief,
                chat: 1,
            });
            chat = {
                current: 1,
                conversations: [{ number: 1, conversation, began: home.now() }],
                reflections: 0,
            };
            chats[chief] = chat;
        }
        await notify(tx, currentOf(chat), words, home.now());
    }
}
const finishing = Symbol("hamlet cleanup");
async function finish(
    home: Home & { [finishing]?: Promise<void> },
    context: Context,
): Promise<void> {
    const previous = home[finishing];
    const next = (async () => {
        await previous;
        await finishHamlets(home, context);
    })();
    home[finishing] = next;
    try {
        await next;
    } finally {
        if (home[finishing] === next) delete home[finishing];
    }
}
async function finishHamlets(home: Home, context: Context): Promise<void> {
    const realm = await home.harness.snapshot(RealmDoc, context);
    for (const place of realm?.hamlets ?? []) {
        try {
            if (place.erased) {
                const dir = join(home.layout.houses, place.id);
                try {
                    await lstat(dir);
                } catch (error) {
                    if (
                        error instanceof Error &&
                        "code" in error &&
                        error.code === "ENOENT"
                    )
                        continue;
                    throw error;
                }
                await removeHouse(home, dir);
                continue;
            }
            const record = await home.harness.snapshot(
                RecordDoc,
                place.id,
                context,
            );
            for (const agent of record?.agents ?? []) {
                const ref = { hamlet: place.id, number: agent.number };
                if (place.removed) await stopAgent(home, ref, context);
                else if (
                    realm?.pending?.to.hamlet !== ref.hamlet ||
                    realm.pending.to.number !== ref.number
                )
                    await buildHouse(home.layout, ref);
            }
        } catch {
            console.error(
                `Hamlet cleanup failed for \`${place.id}\`. Inspect its houses before the next restart; cleanup will be attempted again.`,
            );
        }
    }
}
/** The proposed founding and its origin. */
export type FoundingRequest = {
    hamlet: string;
    mayor: string;
    purpose: string;
    chiefs: string[];
    from: { hamlet: string; vote: number } | null;
};
/** A hamlet founded in one commit. */
export async function foundHamlet(
    home: Home,
    tx: Tx,
    given: FoundingRequest,
): Promise<void> {
    const realm = await tx.doc(RealmDoc);
    const refusal = await refusalOf(
        given,
        realm,
        limitsOf(await tx.doc(LimitsDoc, HOME, HOME)),
        async (id) =>
            (await tx.doc(RecordDoc, id, id)).agents.map((a) => a.name),
    );
    if (refusal) throw new Error(refusal);
    const chiefs = given.from
        ? (realm.hamlets.find((h) => h.id === given.from?.hamlet)?.chiefs ?? [])
        : given.chiefs;
    realm.hamlets.push({
        id: given.hamlet,
        title: null,
        at: null,
        chiefs: [...chiefs],
        from: given.from,
        founded: home.now(),
        removed: null,
        erased: false,
    });
    const record = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
    record.agents = [
        {
            number: 0,
            name: given.mayor,
            purpose: given.purpose.trim(),
            since: home.now(),
            model: null,
        },
    ];
    await tx.doc(WorkDoc, given.hamlet, given.hamlet);
    await tx.doc(LimitsDoc, given.hamlet, given.hamlet);
    await reconfigure(home, tx);
    await tellChiefs(
        home,
        tx,
        given.hamlet,
        `The hamlet \`${given.hamlet}\` was founded under the mayor \`${given.mayor}\`. /hamlet ${given.hamlet} talks to it.`,
    );
}
/** The refusal of a proposed founding. */
export async function mayFound(
    read: DocumentReader,
    given: { hamlet: string; mayor: string; purpose: string },
    context: Context,
): Promise<string | undefined> {
    return refusalOf(
        given,
        await read.snapshot(RealmDoc, context),
        limitsOf(await read.snapshot(LimitsDoc, HOME, context)),
        async (id) =>
            (await read.snapshot(RecordDoc, id, context))?.agents.map(
                (a) => a.name,
            ) ?? [],
    );
}
async function refusalOf(
    given: { hamlet: string; mayor: string; purpose: string },
    realm: Readonly<Realm> | undefined,
    limits: Readonly<Limits>,
    names: (id: string) => Promise<string[]>,
): Promise<string | undefined> {
    try {
        checkName(given.hamlet);
        checkName(given.mayor);
    } catch (error) {
        return error instanceof Error ? error.message : String(error);
    }
    if (!given.purpose.trim())
        return "say what the new hamlet is for in `purpose`";
    if (realm?.hamlets.some((h) => h.id === given.hamlet))
        return `a hamlet named \`${given.hamlet}\` already exists or was used before`;
    const live = liveHamlets(realm);
    if (live.length >= limits.hamlets.most)
        return `the chief's limits allow no more hamlets (${live.length} of ${limits.hamlets.most})`;
    for (const { id } of live) {
        if ((await names(id)).includes(given.mayor))
            return `a new hamlet gets a new mayor, and an agent named \`${given.mayor}\` already exists`;
    }
    return undefined;
}
/** A hamlet closed in one commit. */
export async function removeHamlet(
    home: Home,
    tx: Tx,
    given: { hamlet: string; by: string; reason: string },
): Promise<void> {
    if (given.hamlet === HOME)
        throw new Error("the first hamlet cannot be removed");
    const realm = await tx.doc(RealmDoc);
    refusePendingMove(realm, given.hamlet);
    const place = liveHamlets(realm).find((h) => h.id === given.hamlet);
    if (!place) throw new Error(`no hamlet named \`${given.hamlet}\``);
    for (const agent of (await tx.doc(RecordDoc, given.hamlet, given.hamlet))
        .agents) {
        const ref = { hamlet: given.hamlet, number: agent.number };
        await lapseAgent(tx, ref, home.now());
        for (const grant of grantsOf(await tx.doc(GrantsDoc)).filter(
            (g) => agentKey(g.agent) === agentKey(ref),
        ))
            await revoke(tx, ref, grant.permission, given.by, home.now());
    }
    place.removed = {
        at: home.now(),
        by: given.by,
        reason: given.reason.trim(),
    };
    await reconfigure(home, tx);
    await tellChiefs(
        home,
        tx,
        given.hamlet,
        `The hamlet \`${given.hamlet}\` was removed and its houses archived.`,
    );
}

function refusePendingMove(realm: Readonly<Realm>, id: string): void {
    if (
        realm.pending &&
        [realm.pending.from.hamlet, realm.pending.to.hamlet].includes(id)
    )
        throw new Error(
            `The hamlet \`${id}\` cannot be removed or erased while a house move into or out of it is still being finished. Wait for the move to finish, then try again.`,
        );
}
