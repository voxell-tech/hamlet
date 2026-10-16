import {
    access,
    lstat,
    mkdir,
    readdir,
    readFile,
    rename,
} from "node:fs/promises";
import { dirname, join } from "node:path";
import type { Context } from "@earendil-works/chord";
import { Type } from "@earendil-works/pi-ai";
import {
    type ConversationId,
    defineDoc,
    defineExtension,
    defineTool,
    type ToolExecutionApi,
    type ToolExecutionResult,
    type Tx,
} from "@earendil-works/pi-durable";
import { reconfigure } from "../chats/configure.ts";
import {
    AgentConversationsDoc,
    ChatDoc,
    current,
    WithDoc,
} from "../chats/conversations.ts";
import { PayerDoc } from "../chats/errands.ts";
import { InstructionsDoc } from "../chats/instructions.ts";
import { notify } from "../chats/notice.ts";
import { GrantsDoc, grantsOf, revoke } from "../gate.ts";
import { houseOf } from "../home/layout.ts";
import type { Home, Part } from "../home.ts";
import {
    erase as removeHouse,
    type TreeFailure,
    treeFailureText,
} from "../house.ts";
import { askAgent, letterFailureText, PeersDoc } from "../letters.ts";
import { LimitsDoc } from "../limits/rules.ts";
import { lapseAgent } from "../oversight/waits.ts";
import { CharacterDoc } from "../prompt.ts";
import {
    type AgentRef,
    agentKey,
    type DecisionHeld,
    HOME,
    mayDecide,
    memoised,
    moveMember,
    moving,
    RecordDoc,
    type RecordRefusal,
    recordRefusal,
    refusedTool,
    requireMayor,
    toolText,
} from "../record.ts";
import { err, ok, type Result } from "../result.ts";
import {
    chiefsOf,
    liveHamlets,
    type Realm,
    RealmDoc,
    type RealmEnd,
} from "./places.ts";
import {
    type ResolveFailure,
    resolveAddress,
    resolveFailureText,
} from "./roads.ts";

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
type AgreementResult = Result<
    Agreement,
    | RecordRefusal
    | DecisionHeld
    | MoveFailure
    | { kind: "agreement-address"; failure: ResolveFailure; name: string }
>;
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
    let finishing: Promise<Result<void, MoveFailure>> | undefined;
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
                            (tx) => road(home, tx, api, args, context),
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
                        const before = await finish(context);
                        if (!before.ok)
                            return deliver(home, api, before, context);
                        const result = await decide(
                            api,
                            (tx) => move(home, tx, api, args, context),
                            context,
                        );
                        const after = await finish(context);
                        return deliver(
                            home,
                            api,
                            after.ok ? result : after,
                            context,
                        );
                    },
                }),
            ],
        }),
        async start(context) {
            const recovered = await finish(context);
            if (!recovered.ok)
                throw new Error(moveFailureText(recovered.error));
        },
    };
};
async function decide(
    api: ToolExecutionApi,
    change: (tx: Tx) => Promise<AgreementResult>,
    context: Context,
): Promise<AgreementResult> {
    return memoised(
        api,
        {
            read: async (tx) => {
                const saved = (await tx.doc(AgreementDoc, api.taskId)).result;
                return saved
                    ? ok({
                          ...saved,
                          hamlets: [...saved.hamlets],
                          ask: saved.ask
                              ? { ...saved.ask, to: { ...saved.ask.to } }
                              : null,
                      })
                    : undefined;
            },
            change,
            write: async (tx, result) => {
                if (result.ok)
                    (await tx.doc(AgreementDoc, api.taskId)).result =
                        result.value;
            },
        },
        context,
    );
}
async function mayor(
    home: Home,
    tx: Tx,
    api: ToolExecutionApi,
    _context: Context,
): Promise<Result<AgentRef, RecordRefusal | DecisionHeld>> {
    const caller = await tx.doc(WithDoc, api.conversationId);
    const realm = await tx.doc(RealmDoc);
    const office = await requireMayor(tx, caller);
    if (
        !office.ok ||
        !liveHamlets(realm).some((h) => h.id === caller.hamlet) ||
        liveHamlets(realm).length < 2
    )
        return recordRefusal("agreement", caller.hamlet, {
            kind: "agreement-mayor",
        });
    const held = await mayDecide(tx, caller.hamlet, home.now(), "agreement");
    return held.ok
        ? ok({ hamlet: caller.hamlet, number: caller.number })
        : held;
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
): Promise<AgreementResult> {
    const office = await mayor(home, tx, api, context);
    if (!office.ok) return office;
    const caller = office.value;
    if (!(await tx.doc(LimitsDoc, HOME, HOME)).hamlets.roadsBetween)
        return recordRefusal("agree-road", caller.hamlet, {
            kind: "realm-roads-closed",
        });
    const first = await resolveAddress(tx, caller.hamlet, args.a, context);
    if (!first.ok)
        return err({
            kind: "agreement-address",
            failure: first.error,
            name: args.a,
        });
    const second = await resolveAddress(tx, caller.hamlet, args.b, context);
    if (!second.ok)
        return err({
            kind: "agreement-address",
            failure: second.error,
            name: args.b,
        });
    const a = first.value.ref,
        b = second.value.ref;
    for (const ref of [a, b])
        if (
            (await tx.doc(RecordDoc, ref.hamlet, ref.hamlet)).mayor ===
            ref.number
        )
            return recordRefusal("agree-road", caller.hamlet, {
                kind: "realm-road-office",
            });
    if (a.hamlet === b.hamlet)
        return recordRefusal("agree-road", caller.hamlet, {
            kind: "realm-road-local",
            a: args.a,
            b: args.b,
        });
    if (![a.hamlet, b.hamlet].includes(caller.hamlet))
        return recordRefusal("agree-road", caller.hamlet, {
            kind: "realm-road-neither",
            a: args.a,
            b: args.b,
        });
    const realm = await tx.doc(RealmDoc);
    let road = realm.roads.find(
        (r) => (same(r.a, a) && same(r.b, b)) || (same(r.a, b) && same(r.b, a)),
    );
    if (road?.agreed.includes(a.hamlet) && road.agreed.includes(b.hamlet))
        return recordRefusal("agree-road", caller.hamlet, {
            kind: "road-exists",
            a: args.a,
            b: args.b,
        });
    if (road?.agreed.includes(caller.hamlet))
        return recordRefusal("agree-road", caller.hamlet, {
            kind: "road-agreed",
        });
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
    return ok({
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
    });
}
async function move(
    home: Home,
    tx: Tx,
    api: ToolExecutionApi,
    args: { agent: string; to: string; keep_roads?: string[]; reason: string },
    context: Context,
): Promise<AgreementResult> {
    const office = await mayor(home, tx, api, context);
    if (!office.ok) return office;
    const caller = office.value;
    const realm = await tx.doc(RealmDoc);
    if (realm.pending)
        return recordRefusal("agree-move", caller.hamlet, {
            kind: "move-pending",
        });
    const located = await resolveAddress(
        tx,
        caller.hamlet,
        args.agent,
        context,
    );
    if (!located.ok)
        return err({
            kind: "agreement-address",
            failure: located.error,
            name: args.agent,
        });
    const ref = located.value.ref;
    if (ref.hamlet === args.to)
        return recordRefusal("agree-move", args.to, {
            kind: "move-already-there",
            name: args.agent,
        });
    if (!liveHamlets(realm).some((h) => h.id === args.to))
        return recordRefusal("agree-move", args.to, {
            kind: "record-hamlet-missing",
            removed: false,
        });
    if (![ref.hamlet, args.to].includes(caller.hamlet))
        return recordRefusal("agree-move", caller.hamlet, {
            kind: "move-neither",
            name: args.agent,
        });
    const other = caller.hamlet === ref.hamlet ? args.to : ref.hamlet;
    const held = await mayDecide(tx, other, home.now(), "move");
    if (!held.ok) return held;
    const old = await tx.doc(RecordDoc, ref.hamlet, ref.hamlet),
        target = await tx.doc(RecordDoc, args.to, args.to);
    const permitted = await moving(tx, ref, args.to);
    if (!permitted.ok) return permitted;
    const agent = permitted.value;
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
    const nextNumber = target.next;
    const next = { hamlet: args.to, number: nextNumber };
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
    const moved = await moveMember(tx, ref, args.to, {
        by: `mayor@${caller.hamlet}`,
        reason: args.reason,
        at: home.now(),
    });
    if (!moved.ok)
        throw new Error(
            "The checked membership changed inside one agreement transaction.",
        );
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
/** An unfinished house move whose files need inspection before recovery. */
export type MoveFailure = {
    operation: "finish-move";
    from: AgentRef;
    to: AgentRef;
    retrySafe: false;
} & (
    | { kind: "move-house-occupied" }
    | { kind: "move-house-missing" }
    | { kind: "move-house-tree"; failure: TreeFailure }
);
/** Pending house-move wording for the agreeing mayors and startup diagnostics. */
export function moveFailureText(failure: MoveFailure): string {
    switch (failure.kind) {
        case "move-house-occupied":
            return "The new house is not empty; the move is still pending.";
        case "move-house-missing":
            return "The moving house is missing; the move is still pending.";
        case "move-house-tree":
            return treeFailureText(failure.failure);
    }
    const unhandled: never = failure;
    throw new Error(`Unclassified move recovery: ${unhandled}`);
}
/** Completes a committed house move, leaving refused recovery pending. */
export async function finishMove(
    home: Home,
    context: Context,
): Promise<Result<void, MoveFailure>> {
    const pending = (await home.harness.snapshot(RealmDoc, context))?.pending;
    if (!pending) return ok(undefined);
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
                return err({
                    ...pending,
                    operation: "finish-move",
                    retrySafe: false,
                    kind: "move-house-occupied",
                });
            const names = await readdir(target);
            if (names.some((n) => !["NOTES.md", "skills", "inbox"].includes(n)))
                return err({
                    ...pending,
                    operation: "finish-move",
                    retrySafe: false,
                    kind: "move-house-occupied",
                });
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
                    return err({
                        ...pending,
                        operation: "finish-move",
                        retrySafe: false,
                        kind: "move-house-occupied",
                    });
            }
            const treeOutcome1 = await removeHouse(home, target);
            if (!treeOutcome1.ok)
                return err({
                    ...pending,
                    operation: "finish-move",
                    retrySafe: false,
                    kind: "move-house-tree",
                    failure: treeOutcome1.error,
                });
        }
        await mkdir(dirname(target), { recursive: true });
        await rename(source, target);
    } else if (!(await exists(target)))
        return err({
            ...pending,
            operation: "finish-move",
            retrySafe: false,
            kind: "move-house-missing",
        });
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
    return ok(undefined);
}
async function deliver(
    home: Home,
    api: ToolExecutionApi,
    outcome: AgreementResult,
    context: Context,
): Promise<ToolExecutionResult> {
    if (!outcome.ok) {
        const failure = outcome.error;
        if (
            failure.kind === "move-house-occupied" ||
            failure.kind === "move-house-missing" ||
            failure.kind === "move-house-tree"
        )
            return refusedTool(moveFailureText(failure));
        if (failure.kind === "agreement-address")
            return refusedTool(
                resolveFailureText(failure.failure, failure.name),
            );
        return failure.kind === "unsplit-merge"
            ? { output: [{ type: "text", text: toolText(failure) }] }
            : refusedTool(toolText(failure));
    }
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
        const response = await askAgent(
            home,
            api,
            result.ask.to,
            result.ask.text,
            context,
        );
        answer = response.ok
            ? response.value
            : letterFailureText(response.error);
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
                    await current(
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
