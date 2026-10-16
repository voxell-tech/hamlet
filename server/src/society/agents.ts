import type { Context } from "@earendil-works/chord";
import { type Static, type TSchema, Type } from "@earendil-works/pi-ai";
import {
    defineExtension,
    defineTool,
    type ToolExecutionApi,
    type Tx,
} from "@earendil-works/pi-durable";
import { CharacterDoc } from "../character.ts";
import { WithDoc } from "../chats.ts";
import { choose, commitEdit } from "../choice.ts";
import { type AgentRef, agentKey, checkName, tidyName } from "../hamlet.ts";
import type { Deed, Home, Part } from "../home.ts";
import { buildHouse, localDay } from "../home.ts";
import type { Books } from "../ledger.ts";
import { booksOf, chargeRoad, LedgerDoc } from "../ledger.ts";
import { type Limits, LimitsDoc, limitsOf } from "../limits.ts";
import { add, zero } from "../money.ts";
import { liveHamlets, RealmDoc, roadsOf } from "../realm.ts";
import {
    defineRecordTool,
    type HamletRecord,
    named,
    RecordDoc,
} from "../record.ts";
import { roadAfford } from "../report.ts";
import { SettingsDoc, settingsOf } from "../settings.ts";
import { turnOf } from "../turn.ts";
import { voteOpen } from "./votes.ts";

/** The agent lifecycle and road tools (H27, H3, H29). */
export const Agents: Part = (home) => ({
    extension: defineExtension({
        name: "hamlet.agents",
        tools: [
            mayorTool(home, {
                name: "create_agent",
                description:
                    "Create an agent with a purpose, roads, model and character.",
                parameters: Type.Object({
                    name: Type.String(),
                    purpose: Type.String(),
                    reason: Type.String(),
                    roads: Type.Optional(Type.Array(Type.String())),
                    model: Type.Optional(Type.String()),
                    character: Type.Optional(Type.String()),
                }),
                change: async (tx, args, place, limits, task) => {
                    const name = tidyName(args.name);
                    checkName(name);
                    if (!args.purpose.trim())
                        throw new Error(
                            "say what the agent is for in `purpose`",
                        );
                    for (const h of liveHamlets(await tx.doc(RealmDoc)))
                        if (
                            (await tx.doc(RecordDoc, h.id, h.id)).agents.some(
                                (a) => a.name === name,
                            )
                        )
                            throw new Error(
                                `an agent named \`${name}\` already exists`,
                            );
                    const record = await tx.doc(
                        RecordDoc,
                        place.hamlet,
                        place.hamlet,
                    );
                    if (record.agents.length - 1 >= limits.mostAgents)
                        throw new Error(
                            `the hamlet is full (${limits.mostAgents} agents). Only the chief can raise mostAgents on the hamlet's Bylaws page`,
                        );
                    const number = record.next++;
                    const at = home.now();
                    const allowed =
                        !args.model || limits.models.includes(args.model);
                    record.agents.push({
                        number,
                        name,
                        purpose: args.purpose.trim(),
                        since: at,
                        model: allowed ? args.model || null : null,
                    });
                    record.decisions.push({
                        kind: "created",
                        name,
                        purpose: args.purpose.trim(),
                        agent: number,
                        at,
                        by: "mayor",
                        reason: args.reason.trim(),
                    });
                    if (args.model && allowed)
                        record.decisions.push({
                            kind: "model",
                            agent: number,
                            model: args.model,
                            at,
                            by: "mayor",
                            reason: args.reason.trim(),
                        });
                    const key = agentKey({ hamlet: place.hamlet, number });
                    (await tx.doc(CharacterDoc, key, key)).text =
                        args.character ?? "";
                    const words = [
                        `Created \`${name}\`. Give it work with ask_agent.`,
                    ];
                    if (!allowed)
                        words.push(
                            `This hamlet may not run \`${args.model}\`; the agent runs the hamlet's own model.`,
                        );
                    for (const peer of args.roads ?? []) {
                        const refusal =
                            roadRefusal(record, limits, name, peer, false) ??
                            (await roadPrice(home, tx, place.hamlet, limits));
                        if (refusal)
                            words.push(
                                `Road to \`${peer}\` refused: ${refusal}.`,
                            );
                        else {
                            await lay(
                                home,
                                tx,
                                record,
                                place,
                                limits,
                                task,
                                "mayor",
                                name,
                                peer,
                                args.reason,
                            );
                        }
                    }
                    return words.join("\n");
                },
                after: async (place, args, context) => {
                    const record = await home.harness.snapshot(
                        RecordDoc,
                        place.hamlet,
                        context,
                    );
                    const agent = record?.agents.find(
                        (a) => a.name === tidyName(args.name),
                    );
                    if (agent)
                        await buildHouse(home.layout, {
                            hamlet: place.hamlet,
                            number: agent.number,
                        });
                },
            }),
            mayorTool(home, {
                name: "build_road",
                description:
                    "Build a road between two agents, charged to the hamlet.",
                parameters: Type.Object({
                    a: Type.String(),
                    b: Type.String(),
                    reason: Type.String(),
                }),
                change: async (tx, args, place, limits, task) => {
                    const record = await tx.doc(
                        RecordDoc,
                        place.hamlet,
                        place.hamlet,
                    );
                    const refusal =
                        roadRefusal(record, limits, args.a, args.b, false) ??
                        (await roadPrice(home, tx, place.hamlet, limits));
                    if (refusal) throw new Error(refusal);
                    await lay(
                        home,
                        tx,
                        record,
                        place,
                        limits,
                        task,
                        "mayor",
                        args.a,
                        args.b,
                        args.reason,
                    );
                    return `Built a road between \`${args.a}\` and \`${args.b}\`.`;
                },
            }),
            mayorTool(home, {
                name: "set_model",
                description:
                    "Choose an allowed model for an agent, or empty for the hamlet's own.",
                parameters: Type.Object({
                    name: Type.String(),
                    model: Type.String(),
                    reason: Type.String(),
                }),
                change: async (tx, args, place, limits) => {
                    const record = await tx.doc(
                        RecordDoc,
                        place.hamlet,
                        place.hamlet,
                    );
                    const agent = named(record, args.name);
                    if (!agent || agent.number === record.mayor)
                        throw new Error(`no agent named \`${args.name}\``);
                    if (Object.hasOwn(limits.pinned, agent.name))
                        throw new Error(
                            `the chief has set the model of \`${agent.name}\`; only the chief can change it`,
                        );
                    if (args.model && !limits.models.includes(args.model))
                        throw new Error(
                            `this hamlet may not run \`${args.model}\`. You may choose: ${limits.models.join(", ") || "the hamlet's own model"}`,
                        );
                    const before = record.decisions.length;
                    await commitEdit(home, tx, {
                        by: "mayor",
                        reason: args.reason.trim(),
                        change: () => {
                            agent.model = args.model || null;
                        },
                    });
                    if (record.decisions.length === before)
                        record.decisions.push({
                            kind: "model",
                            agent: agent.number,
                            model: choose(
                                limits,
                                settingsOf(
                                    await tx.doc(SettingsDoc),
                                    home.overrides,
                                ),
                                { ...agent, mayor: false },
                            ),
                            at: home.now(),
                            by: "mayor",
                            reason: args.reason.trim(),
                        });
                    return `\`${agent.name}\` now runs ${args.model || "the hamlet's own model"}.`;
                },
            }),
        ],
    }),
});

function mayorTool<P extends TSchema>(
    home: Home,
    tool: {
        name: string;
        description: string;
        parameters: P;
        change(
            tx: Tx,
            args: Static<P>,
            place: AgentRef,
            limits: Readonly<Limits>,
            task: string | null,
        ): Promise<string>;
        after?(
            place: AgentRef,
            args: Static<P>,
            context: Context,
        ): Promise<void>;
    },
) {
    return defineTool({
        name: tool.name,
        description: tool.description,
        parameters: tool.parameters,
        replay: "safe",
        execute: async (args, api: ToolExecutionApi, context) => {
            try {
                const place = await api.snapshot(
                    WithDoc,
                    api.conversationId,
                    context,
                );
                if (!place) throw new Error("Only the mayor may do that.");
                const record = await api.snapshot(
                    RecordDoc,
                    place.hamlet,
                    context,
                );
                if (record?.mayor !== place.number)
                    throw new Error("Only the mayor may do that.");
                const held = await voteOpen(
                    api,
                    place.hamlet,
                    home.now(),
                    context,
                );
                if (held) throw new Error(held);
                const limits = limitsOf(
                    await api.snapshot(LimitsDoc, place.hamlet, context),
                );
                const task =
                    (await turnOf(home.harness, api, context))?.task ?? null;
                await booksOf(
                    home,
                    place.hamlet,
                    limits.budget.period,
                    task,
                    context,
                );
                const result = await defineRecordTool({
                    ...tool,
                    change: async (tx, args) => {
                        const current = await tx.doc(
                            RecordDoc,
                            place.hamlet,
                            place.hamlet,
                        );
                        if (current.mayor !== place.number)
                            return "Only the mayor may do that.";
                        return tool.change(
                            tx,
                            args,
                            place,
                            await tx.doc(LimitsDoc, place.hamlet, place.hamlet),
                            task,
                        );
                    },
                }).execute(args, api, context);
                await tool.after?.(place, args, context);
                return result;
            } catch (error) {
                return {
                    output: [
                        {
                            type: "text" as const,
                            text:
                                error instanceof Error
                                    ? error.message
                                    : String(error),
                        },
                    ],
                };
            }
        },
    });
}

function roadRefusal(
    record: HamletRecord,
    limits: Readonly<Limits>,
    a: string,
    b: string,
    chief: boolean,
): string | undefined {
    const aa = named(record, a),
        bb = named(record, b);
    if (aa?.number === record.mayor || bb?.number === record.mayor)
        return "every agent already has a road to the mayor";
    for (const [name, agent] of [
        [a, aa],
        [b, bb],
    ] as const)
        if (!agent) return `no agent named \`${name}\` in this hamlet`;
    if (aa?.number === bb?.number) return "a road needs two different agents";
    if (
        record.roads.some(
            (r) =>
                (r.a === aa?.number && r.b === bb?.number) ||
                (r.b === aa?.number && r.a === bb?.number),
        )
    )
        return `\`${a}\` and \`${b}\` already have a road`;
    const built = roadsOf(record).filter(
        (r) => r.a !== "office" && r.b !== "office",
    ).length;
    if (built >= limits.mostRoads)
        return `the hamlet has all the roads ${chief ? "your limits allow" : "the chief allows"} (${built}). Only the chief can raise mostRoads on the hamlet's Bylaws page`;
    return undefined;
}

async function lay(
    home: Home,
    tx: Tx,
    record: HamletRecord,
    place: AgentRef,
    limits: Readonly<Limits>,
    task: string | null,
    by: string,
    a: string,
    b: string,
    reason: string,
) {
    const aa = named(record, a),
        bb = named(record, b);
    if (!aa || !bb) throw new Error("The agents have left.");
    const at = home.now();
    await chargeRoad(tx, place.hamlet, {
        task,
        agent: place.number,
        at,
        limits,
    });
    record.roads.push({
        a: aa.number,
        b: bb.number,
        messages: 0,
        lastFrom: null,
        lastUsed: 0,
        since: at,
        by,
    });
    record.decisions.push({
        kind: "road_built",
        a: aa.number,
        b: bb.number,
        at,
        by,
        reason: reason.trim(),
    });
}

/** A chief's road building deed. */
export const road: Deed = async (home, given, context) => {
    const [a, b, ...reason] = given.rest.trim().split(/\s+/);
    if (!a || !b) throw new Error("Use /road a b [reason].");
    const limits = limitsOf(
        await home.harness.snapshot(LimitsDoc, given.hamlet, context),
    );
    await booksOf(home, given.hamlet, limits.budget.period, null, context);
    return home.harness.commit(async (tx) => {
        const record = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
        const limits = await tx.doc(LimitsDoc, given.hamlet, given.hamlet);
        const refusal =
            roadRefusal(record, limits, a, b, true) ??
            (await roadPrice(home, tx, given.hamlet, limits));
        if (refusal) throw new Error(refusal);
        await lay(
            home,
            tx,
            record,
            { hamlet: given.hamlet, number: record.mayor },
            limits,
            null,
            `chief:${given.chief}`,
            a,
            b,
            reason.join(" "),
        );
        return `Built a road between \`${a}\` and \`${b}\`.`;
    }, context);
};

/** A chief's road removal deed. */
export const unroad: Deed = async (home, given, context) => {
    const [a, b, ...reason] = given.rest.trim().split(/\s+/);
    if (!a || !b) throw new Error("Use /unroad a b [reason].");
    return home.harness.commit(async (tx) => {
        const record = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
        const aa = named(record, a),
            bb = named(record, b);
        if (aa?.number === record.mayor || bb?.number === record.mayor)
            throw new Error(
                "an agent's road to the mayor stays for as long as the agent does",
            );
        const index = record.roads.findIndex(
            (r) =>
                aa &&
                bb &&
                ((r.a === aa.number && r.b === bb.number) ||
                    (r.a === bb.number && r.b === aa.number)),
        );
        if (index < 0) throw new Error(`\`${a}\` and \`${b}\` have no road`);
        const removed = record.roads.splice(index, 1)[0];
        if (removed)
            record.decisions.push({
                kind: "road_removed",
                a: removed.a,
                b: removed.b,
                at: home.now(),
                by: `chief:${given.chief}`,
                reason: reason.join(" "),
            });
        return `Took up the road between \`${a}\` and \`${b}\`. Nothing is refunded.`;
    }, context);
};

async function roadPrice(
    home: Home,
    tx: Tx,
    hamlet: string,
    limits: Readonly<Limits>,
): Promise<string | undefined> {
    // The affordability read shares the charge's transaction, including competing roads.
    const books: Books = { hamlet: zero(), agents: {}, task: zero() };
    const today = new Date(home.now());
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
    return roadAfford(books, limits);
}
