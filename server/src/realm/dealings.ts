import {
    access,
    lstat,
    mkdir,
    readdir,
    readFile,
    rename,
} from "node:fs/promises";
import { dirname, join } from "node:path";
import type { Context, JsonValue } from "@earendil-works/chord";
import { Type } from "@earendil-works/pi-ai";
import {
    type ConversationId,
    type DocDefinition,
    type DocFamilyDefinition,
    type DocFamilyToken,
    type DocToken,
    type DocumentReader,
    defineDoc,
    defineExtension,
    defineTool,
    type JsonObject,
    type SessionDocFamilyToken,
    type ToolExecutionApi,
    type Tx,
} from "@earendil-works/pi-durable";
import {
    AgentConversationsDoc,
    ChatDoc,
    chatOf,
    WithDoc,
} from "../chats/conversations.ts";
import { PayerDoc } from "../chats/errands.ts";
import { InstructionsDoc } from "../chats/instructions.ts";
import { notify } from "../chats/notice.ts";
import { GrantsDoc, grantsOf, revoke } from "../gate.ts";
import { houseOf } from "../home/layout.ts";
import type { Home, Part } from "../home.ts";
import { removeHouse } from "../house.ts";
import { askAgent, PeersDoc } from "../letters.ts";
import { LimitsDoc } from "../limits/rules.ts";
import { reconfigure } from "../models.ts";
import { lapseAgent } from "../oversight/waits.ts";
import { CharacterDoc } from "../prompt.ts";
import { type AgentRef, agentKey, HOME, RecordDoc } from "../record.ts";
import { VotesDoc, voteOpen } from "../votes.ts";
import {
    chiefsOf,
    liveHamlets,
    type Realm,
    RealmDoc,
    type RealmEnd,
} from "./places.ts";

const MOVE = Type.Object({
    agent: Type.String(),
    to: Type.String(),
    keep_roads: Type.Optional(Type.Array(Type.String())),
    reason: Type.String(),
});
const ROAD = Type.Object({
    a: Type.String(),
    b: Type.String(),
    reason: Type.String(),
});
type Agreement = {
    words: string;
    hamlets: string[];
    notice?: boolean;
    ask: { to: AgentRef; text: string } | null;
};
type MoveFailure = {
    kind: "unsplit-merge";
    operation: "agree-move";
    agent: AgentRef;
    name: string;
    absorbed: string;
    archive: string;
    retrySafe: true;
    recovery: "split-or-erase";
};
type AgreementResult =
    | { ok: true; value: Agreement }
    | { ok: false; error: MoveFailure };
function moveFailureText(error: MoveFailure): string {
    switch (error.kind) {
        case "unsplit-merge":
            return `Cannot move \`${error.name}\`: \`${error.absorbed}\` is still merged into it. Ask a chief to split or erase that archive first: /restore ${error.archive} or /erase ${error.archive}.`;
    }
    const unhandled: never = error.kind;
    throw new Error(`Unclassified move failure: ${unhandled}`);
}
const AgreementDoc = defineDoc<{ result: Agreement | null; notified: boolean }>(
    {
        kind: "hamlet.agreement-call",
        version: 1,
        scope: "task",
        initial: () => ({ result: null, notified: false }),
    },
);
/** Agreements between hamlets. */
export const Dealings: Part = (home) => {
    let finishing: Promise<void> | undefined;
    const finish = (context: Context) =>
        (finishing ??= finishMove(home, context).finally(() => {
            finishing = undefined;
        }));
    return {
        extension: defineExtension({
            name: "hamlet.dealings",
            tools: [
                defineTool({
                    name: "agree_road",
                    description:
                        "Agree to a road between agents of different hamlets. Both mayors must agree.",
                    parameters: ROAD,
                    replay: "safe",
                    execute: async (args, api, context) => {
                        const result = await decide(
                            api,
                            async (tx) => ({
                                ok: true,
                                value: await road(home, tx, api, args, context),
                            }),
                            context,
                        );
                        return deliver(home, api, result, context);
                    },
                }),
                defineTool({
                    name: "agree_move",
                    description:
                        "Agree that an agent moves with its house. Only roads both mayors name are kept.",
                    parameters: MOVE,
                    replay: "safe",
                    execute: async (args, api, context) => {
                        await finish(context);
                        const result = await decide(
                            api,
                            (tx) => move(home, tx, api, args, context),
                            context,
                        );
                        await finish(context);
                        return deliver(home, api, result, context);
                    },
                }),
            ],
        }),
        async start(context) {
            await finish(context);
        },
    };
};
async function decide(
    api: ToolExecutionApi,
    change: (tx: Tx) => Promise<AgreementResult>,
    context: Context,
): Promise<AgreementResult> {
    return api.commit(async (tx) => {
        const call = await tx.doc(AgreementDoc, api.taskId);
        if (!call.result) {
            const outcome = await change(tx);
            if (!outcome.ok) return outcome;
            call.result = outcome.value;
        }
        return {
            ok: true,
            value: {
                ...call.result,
                hamlets: [...call.result.hamlets],
                ask: call.result.ask
                    ? {
                          to: { ...call.result.ask.to },
                          text: call.result.ask.text,
                      }
                    : null,
            },
        };
    }, context);
}
async function mayor(
    home: Home,
    tx: Tx,
    api: ToolExecutionApi,
    context: Context,
) {
    const caller = await tx.doc(WithDoc, api.conversationId);
    const realm = await tx.doc(RealmDoc);
    const record = await tx.doc(RecordDoc, caller.hamlet, caller.hamlet);
    if (
        record.mayor !== caller.number ||
        !liveHamlets(realm).some((h) => h.id === caller.hamlet) ||
        liveHamlets(realm).length < 2
    )
        throw new Error(
            "only the mayor can do this, and you are not the mayor of this hamlet",
        );
    const held = await voteOpen(
        hamletReader(tx),
        caller.hamlet,
        home.now(),
        context,
    );
    if (held) throw new Error(held);
    return caller;
}
async function locate(tx: Tx, address: string): Promise<AgentRef> {
    const [name, place, extra] = address.split("@");
    if (extra !== undefined) throw new Error(`no agent named \`${address}\``);
    for (const { id } of liveHamlets(await tx.doc(RealmDoc))) {
        if (place !== undefined && id !== place) continue;
        const record = await tx.doc(RecordDoc, id, id);
        const found = record.agents.find(
            (a) =>
                a.name === name ||
                (name === "mayor" && a.number === record.mayor),
        );
        if (found) return { hamlet: id, number: found.number };
    }
    throw new Error(`no agent named \`${address}\``);
}
function same(a: RealmEnd, b: RealmEnd): boolean {
    return "office" in a
        ? "office" in b && a.office === b.office
        : !("office" in b) && a.hamlet === b.hamlet && a.number === b.number;
}
async function road(
    home: Home,
    tx: Tx,
    api: ToolExecutionApi,
    args: { a: string; b: string; reason: string },
    context: Context,
): Promise<Agreement> {
    const caller = await mayor(home, tx, api, context);
    if (!(await tx.doc(LimitsDoc, HOME, HOME)).hamlets.roadsBetween)
        throw new Error("the chief's limits allow no roads between hamlets");
    const a = await locate(tx, args.a),
        b = await locate(tx, args.b);
    for (const ref of [a, b])
        if (
            (await tx.doc(RecordDoc, ref.hamlet, ref.hamlet)).mayor ===
            ref.number
        )
            throw new Error(
                "name two agents; mayors already have roads to each other",
            );
    if (a.hamlet === b.hamlet)
        throw new Error(
            `\`${args.a}\` and \`${args.b}\` live in the same hamlet; its mayor builds that road with build_road`,
        );
    if (![a.hamlet, b.hamlet].includes(caller.hamlet))
        throw new Error(
            `neither \`${args.a}\` nor \`${args.b}\` lives in your hamlet`,
        );
    const realm = await tx.doc(RealmDoc);
    let road = realm.roads.find(
        (r) => (same(r.a, a) && same(r.b, b)) || (same(r.a, b) && same(r.b, a)),
    );
    if (road?.agreed.includes(a.hamlet) && road.agreed.includes(b.hamlet))
        throw new Error(`\`${args.a}\` and \`${args.b}\` already have a road`);
    if (road?.agreed.includes(caller.hamlet))
        throw new Error(
            "you have agreed already; the road waits for the other mayor",
        );
    if (!road) {
        road = {
            a,
            b,
            agreed: [caller.hamlet],
            reason: args.reason.trim(),
            messages: 0,
            lastFrom: null,
            lastUsed: 0,
        };
        realm.roads.push(road);
    } else road.agreed.push(caller.hamlet);
    const built =
        road.agreed.includes(a.hamlet) && road.agreed.includes(b.hamlet);
    const other = caller.hamlet === a.hamlet ? b.hamlet : a.hamlet;
    const words = built
        ? `A road now joins \`${args.a}\` and \`${args.b}\` across hamlets.`
        : `One mayor agreed to a road between \`${args.a}\` and \`${args.b}\`; the other has not yet.`;
    return {
        words,
        hamlets: [a.hamlet, b.hamlet],
        ask: built
            ? null
            : {
                  to: {
                      hamlet: other,
                      number: (await tx.doc(RecordDoc, other, other)).mayor,
                  },
                  text: `The mayor of \`${caller.hamlet}\` agrees to a road between \`${args.a}\` and \`${args.b}\`. Its reason: ${args.reason}\nDecide. To agree, call agree_road with the same two names and your reason. To refuse, say why in a sentence.`,
              },
    };
}
async function move(
    home: Home,
    tx: Tx,
    api: ToolExecutionApi,
    args: { agent: string; to: string; keep_roads?: string[]; reason: string },
    context: Context,
): Promise<AgreementResult> {
    const caller = await mayor(home, tx, api, context);
    const realm = await tx.doc(RealmDoc);
    if (realm.pending)
        throw new Error(
            "A house move is still being finished. Try again shortly.",
        );
    const ref = await locate(tx, args.agent);
    if (ref.hamlet === args.to)
        throw new Error(`\`${args.agent}\` already lives in \`${args.to}\``);
    if (!liveHamlets(realm).some((h) => h.id === args.to))
        throw new Error(`no hamlet named \`${args.to}\``);
    if (![ref.hamlet, args.to].includes(caller.hamlet))
        throw new Error(
            `only the mayor \`${args.agent}\` leaves and the mayor it joins decide this`,
        );
    const other = caller.hamlet === ref.hamlet ? args.to : ref.hamlet;
    const held = await voteOpen(hamletReader(tx), other, home.now(), context);
    if (held) throw new Error(held);
    const old = await tx.doc(RecordDoc, ref.hamlet, ref.hamlet),
        target = await tx.doc(RecordDoc, args.to, args.to);
    if (old.mayor === ref.number)
        throw new Error("a mayor never leaves its hamlet");
    if (
        target.agents.length >=
        (await tx.doc(LimitsDoc, args.to, args.to)).mostAgents
    )
        throw new Error(`the hamlet \`${args.to}\` is full`);
    const agent = old.agents.find((a) => a.number === ref.number);
    if (!agent) throw new Error(`no agent named \`${args.agent}\``);
    const merged = old.archive.find(
        (entry) =>
            entry.back === null && entry.merge?.kept.number === ref.number,
    );
    if (merged)
        return {
            ok: false,
            error: {
                kind: "unsplit-merge",
                operation: "agree-move",
                agent: ref,
                name: agent.name,
                absorbed: merged.agent.name,
                archive: merged.id,
                retrySafe: true,
                recovery: "split-or-erase",
            },
        };
    const keep = old.agents
        .filter(
            (a) =>
                args.keep_roads?.includes(a.name) &&
                a.number !== ref.number &&
                a.number !== old.mayor,
        )
        .map((a) => a.number);
    let proposal = realm.moves.find(
        (m) => same(m.agent, ref) && m.to === args.to,
    );
    const agreement = {
        hamlet: caller.hamlet,
        keep,
        reason: args.reason.trim(),
        at: home.now(),
    };
    if (!proposal) {
        realm.moves.push({ agent: ref, to: args.to, agreed: [agreement] });
        proposal = realm.moves.find(
            (m) => same(m.agent, ref) && m.to === args.to,
        );
    } else {
        proposal.agreed = proposal.agreed.filter(
            (a) => a.hamlet !== caller.hamlet,
        );
        proposal.agreed.push(agreement);
    }
    const leaving = proposal?.agreed.find((a) => a.hamlet === ref.hamlet),
        joining = proposal?.agreed.find((a) => a.hamlet === args.to);
    if (!leaving || !joining) {
        const other = caller.hamlet === ref.hamlet ? args.to : ref.hamlet;
        return {
            ok: true,
            value: {
                hamlets: [ref.hamlet, args.to],
                words: `You agreed that \`${agent.name}\` moves to \`${args.to}\`. The other mayor has not yet.`,
                ask: {
                    to: {
                        hamlet: other,
                        number: (await tx.doc(RecordDoc, other, other)).mayor,
                    },
                    text: `The mayor of \`${caller.hamlet}\` agrees that \`${agent.name}\` moves from \`${ref.hamlet}\` to \`${args.to}\` with its house, keeping its roads to [${args.keep_roads?.join(", ") ?? ""}]. Its reason: ${args.reason}\nDecide. To agree, call agree_move with the same agent and hamlet, and the roads you agree it keeps. To refuse, say why in a sentence.`,
                },
            },
        };
    }
    const next = { hamlet: args.to, number: target.next++ };
    const kept: string[] = [],
        lapsed: string[] = [];
    const retained = old.roads.filter(
        (r) => r.a === ref.number || r.b === ref.number,
    );
    realm.roads = realm.roads.filter((r) => !same(r.a, ref) && !same(r.b, ref));
    for (const road of retained) {
        const peer = road.a === ref.number ? road.b : road.a;
        if (peer === "office" || peer === old.mayor) continue;
        const name = old.agents.find((a) => a.number === peer)?.name;
        if (!name) continue;
        if (leaving.keep.includes(peer) && joining.keep.includes(peer)) {
            kept.push(name);
            realm.roads.push({
                a: next,
                b: { hamlet: ref.hamlet, number: peer },
                agreed: [ref.hamlet, args.to],
                reason: `kept when \`${agent.name}\` moved`,
                messages: road.messages,
                lastFrom:
                    road.lastFrom === ref.number
                        ? next
                        : road.lastFrom === null
                          ? null
                          : { hamlet: ref.hamlet, number: peer },
                lastUsed: road.lastUsed,
            });
        } else lapsed.push(name);
    }
    const name = agent.name;
    target.agents.push({ ...agent, number: next.number });
    old.agents = old.agents.filter((a) => a.number !== ref.number);
    old.roads = old.roads.filter(
        (r) => r.a !== ref.number && r.b !== ref.number,
    );
    old.decisions.push({
        kind: "moved_out",
        agent: ref.number,
        name,
        hamlet: args.to,
        at: home.now(),
        by: `mayor@${caller.hamlet}`,
        reason: args.reason,
    });
    target.decisions.push({
        kind: "moved_in",
        agent: next.number,
        name,
        hamlet: ref.hamlet,
        at: home.now(),
        by: `mayor@${caller.hamlet}`,
        reason: args.reason,
    });
    await transfer(home, tx, ref, next);
    realm.moves = realm.moves.filter((m) => !same(m.agent, ref));
    realm.pending = { from: ref, to: next };
    return {
        ok: true,
        value: {
            notice: false,
            hamlets: [ref.hamlet, args.to],
            words: `\`${name}\` moved from \`${ref.hamlet}\` to \`${args.to}\` with its house. Roads kept: [${kept.join(", ")}]. Roads that lapsed: [${lapsed.join(", ")}].`,
            ask: null,
        },
    };
}
async function transfer(
    home: Home,
    tx: Tx,
    from: AgentRef,
    to: AgentRef,
): Promise<void> {
    const before = agentKey(from),
        after = agentKey(to);
    const character = await tx.doc(CharacterDoc, before, before);
    (await tx.doc(CharacterDoc, after, after)).text = character.text;
    character.text = "";
    const chats = await tx.doc(ChatDoc, before, before);
    (await tx.doc(ChatDoc, after, after)).chats = chats.chats;
    chats.chats = {};
    const peers = await tx.doc(PeersDoc, before, before);
    Object.assign(await tx.doc(PeersDoc, after, after), {
        letters: peers.letters,
        questions: peers.questions,
    });
    peers.letters = {};
    peers.questions = {};
    const instructions = await tx.doc(InstructionsDoc, before, before);
    (await tx.doc(InstructionsDoc, after, after)).given = instructions.given;
    instructions.given = [];
    const index = await tx.doc(AgentConversationsDoc, before, before);
    (await tx.doc(AgentConversationsDoc, after, after)).ids = index.ids;
    for (const id of index.ids) Object.assign(await tx.doc(WithDoc, id), to);
    index.ids = [];
    for (const place of (await tx.doc(RealmDoc)).hamlets) {
        const record = await tx.doc(RecordDoc, place.id, place.id);
        for (const agent of [
            ...record.agents,
            ...record.archive.map((a) => a.agent),
        ]) {
            const key = agentKey({ hamlet: place.id, number: agent.number });
            const other = await tx.doc(PeersDoc, key, key);
            for (const table of [other.letters, other.questions]) {
                const conversation = table[before];
                if (conversation !== undefined) {
                    table[after] = conversation;
                    delete table[before];
                    const payer = await tx.doc(PayerDoc, conversation);
                    if (payer.hamlet === from.hamlet) payer.hamlet = to.hamlet;
                }
            }
        }
    }
    await lapseAgent(tx, from, home.now());
    for (const grant of grantsOf(await tx.doc(GrantsDoc)).filter(
        (g) => agentKey(g.agent) === before,
    ))
        await revoke(tx, from, grant.permission, "move", home.now());
}
async function finishMove(home: Home, context: Context): Promise<void> {
    const pending = (await home.harness.snapshot(RealmDoc, context))?.pending;
    if (!pending) return;
    const source = houseOf(home.layout, pending.from),
        target = houseOf(home.layout, pending.to);
    const exists = async (path: string) => {
        try {
            await access(path);
            return true;
        } catch (error) {
            if ((error as NodeJS.ErrnoException).code === "ENOENT")
                return false;
            throw error;
        }
    };
    if (await exists(source)) {
        if (await exists(target)) {
            const info = await lstat(target);
            if (!info.isDirectory() || info.isSymbolicLink())
                throw new Error(
                    "The new house is not empty; the move is still pending.",
                );
            const names = await readdir(target);
            if (names.some((n) => !["NOTES.md", "skills", "inbox"].includes(n)))
                throw new Error(
                    "The new house is not empty; the move is still pending.",
                );
            for (const name of names) {
                const path = join(target, name),
                    info = await lstat(path);
                if (
                    info.isSymbolicLink() ||
                    (name === "NOTES.md"
                        ? !info.isFile() ||
                          (await readFile(path, "utf8")) !== "# Notes\n"
                        : !info.isDirectory() ||
                          (await readdir(path)).length !== 0)
                )
                    throw new Error(
                        "The new house is not empty; the move is still pending.",
                    );
            }
            await removeHouse(home, target);
        }
        await mkdir(dirname(target), { recursive: true });
        await rename(source, target);
    } else if (!(await exists(target)))
        throw new Error(
            "The moving house is missing; the move is still pending.",
        );
    const chats = await chiefChats(
        home,
        [pending.from.hamlet, pending.to.hamlet],
        context,
    );
    await home.harness.commit(async (tx) => {
        const realm = await tx.doc(RealmDoc);
        if (
            !realm.pending ||
            !same(realm.pending.from, pending.from) ||
            !same(realm.pending.to, pending.to)
        )
            return;
        const agent = (
            await tx.doc(RecordDoc, pending.to.hamlet, pending.to.hamlet)
        ).agents.find((a) => a.number === pending.to.number);
        for (const chat of chats)
            await notify(
                tx,
                chat,
                `\`${agent?.name}\` moved from \`${pending.from.hamlet}\` to \`${pending.to.hamlet}\` with its house.`,
                home.now(),
            );
        realm.pending = null;
        await reconfigure(home, tx);
    }, context);
}
async function deliver(
    home: Home,
    api: ToolExecutionApi,
    outcome: AgreementResult,
    context: Context,
) {
    if (!outcome.ok)
        return {
            output: [
                { type: "text" as const, text: moveFailureText(outcome.error) },
            ],
        };
    const result = outcome.value;
    const chats =
        result.notice === false
            ? []
            : await chiefChats(home, result.hamlets, context);
    await api.commit(async (tx) => {
        const call = await tx.doc(AgreementDoc, api.taskId);
        if (call.notified) return;
        for (const chat of chats)
            await notify(tx, chat, result.words, home.now());
        call.notified = true;
    }, context);
    let answer: string | undefined;
    if (result.ask) {
        try {
            answer = await askAgent(
                home,
                api,
                result.ask.to,
                result.ask.text,
                context,
            );
        } catch (error) {
            answer = error instanceof Error ? error.message : String(error);
        }
    }
    return {
        output: [
            {
                type: "text" as const,
                text: answer ? `${result.words}\n${answer}` : result.words,
            },
        ],
    };
}
async function chiefChats(
    home: Home,
    hamlets: string[],
    context: Context,
): Promise<ConversationId[]> {
    const chats: ConversationId[] = [];
    const realm = await home.harness.snapshot(RealmDoc, context);
    for (const hamlet of hamlets) {
        const record = await home.harness.snapshot(RecordDoc, hamlet, context);
        const place = realm?.hamlets.find((h) => h.id === hamlet);
        if (!record || !place) continue;
        for (const chief of chiefsOf(realm, hamlet, home.chief))
            chats.push(
                (
                    await chatOf(
                        home,
                        { agent: { hamlet, number: record.mayor }, chief },
                        context,
                    )
                ).id,
            );
    }
    return chats;
}
/** The hamlet's agreements still waiting on another mayor. */
export async function awaiting(
    home: Home,
    hamlet: string,
    context: Context,
): Promise<string | undefined> {
    const realm = await home.harness.snapshot(RealmDoc, context);
    if (!realm) return undefined;
    const lines: string[] = [];
    for (const road of realm.roads) {
        if ("office" in road.a || "office" in road.b) continue;
        if (
            ![road.a.hamlet, road.b.hamlet].includes(hamlet) ||
            road.agreed.includes(hamlet) ||
            !road.agreed.length
        )
            continue;
        const a = await nameOf(home, realm, road.a, context),
            b = await nameOf(home, realm, road.b, context);
        if (a && b)
            lines.push(
                `- the mayor of \`${road.agreed[0]}\` agreed to a road between \`${a}\` and \`${b}\`: ${road.reason} Agree with agree_road, or leave it.`,
            );
    }
    for (const move of realm.moves) {
        if (
            ![move.agent.hamlet, move.to].includes(hamlet) ||
            move.agreed.some((a) => a.hamlet === hamlet) ||
            !move.agreed.length
        )
            continue;
        const name = await nameOf(home, realm, move.agent, context),
            agreed = move.agreed[0];
        if (
            !name ||
            !agreed ||
            !liveHamlets(realm).some((h) => h.id === move.to) ||
            !liveHamlets(realm).some((h) => h.id === agreed.hamlet)
        )
            continue;
        const record = await home.harness.snapshot(
            RecordDoc,
            move.agent.hamlet,
            context,
        );
        const keep = agreed.keep.flatMap(
            (n) =>
                record?.agents
                    .filter((a) => a.number === n)
                    .map((a) => a.name) ?? [],
        );
        lines.push(
            `- the mayor of \`${agreed.hamlet}\` agreed that \`${name}\` moves to \`${move.to}\`, keeping its roads to [${keep.join(", ")}]: ${agreed.reason} Agree with agree_move, or leave it.`,
        );
    }
    return lines.length ? lines.join("\n") : undefined;
}
async function nameOf(
    home: Home,
    realm: Readonly<Realm>,
    ref: AgentRef,
    context: Context,
): Promise<string | undefined> {
    if (!liveHamlets(realm).some((h) => h.id === ref.hamlet)) return undefined;
    return (
        await home.harness.snapshot(RecordDoc, ref.hamlet, context)
    )?.agents.find((a) => a.number === ref.number)?.name;
}

function hamletReader(tx: Tx): DocumentReader {
    return {
        snapshot: async <T extends JsonObject, I extends JsonValue>(
            token:
                | DocToken<T, DocDefinition<T>>
                | DocFamilyToken<T, I, DocFamilyDefinition<T, I>>,
            ...args: (string | number | Context)[]
        ): Promise<Readonly<T> | undefined> => {
            const key = args[0];
            if (
                ![
                    LimitsDoc.definition,
                    VotesDoc.definition,
                    RecordDoc.definition,
                ].some((definition) =>
                    Object.is(definition, token.definition),
                ) ||
                typeof key !== "string"
            )
                throw new Error(
                    "Vote checks require current hamlet documents.",
                );
            // SAFETY: these three library tokens all use a hamlet string seed; generic token identity does not narrow I.
            // Verified by: dealings.test.ts, stored-vote agreement tests.
            const family = token as SessionDocFamilyToken<T, string>;
            const value = await tx.doc(family, key, key);
            // SAFETY: pi-durable returns the draft of T; TypeScript cannot reduce its recursive Draft<T> to Readonly<T>.
            // Verified by: dealings.test.ts, stored-vote agreement tests.
            return value as Readonly<T>;
        },
        snapshotAsOf: async () => {
            throw new Error("Vote checks require current hamlet documents.");
        },
    };
}
