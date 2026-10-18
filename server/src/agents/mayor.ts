import { Type } from "@earendil-works/pi-ai";
import { defineExtension } from "@earendil-works/pi-durable";
import { commitEdit } from "../chats/configure.ts";
import type { Part } from "../home.ts";
import {
    admit,
    defineMayorTool,
    layRoad,
    named,
    type RecordDeed,
    RecordDoc,
    recordRefusal,
    recordRefusalText,
    takeUpRoad,
    tidyName,
} from "../record.ts";
import { ok } from "../result.ts";
import { choose, SettingsDoc, settingsOf } from "../settings.ts";
import { reconcile } from "../spend/ledger.ts";
/** The agent lifecycle and road tools (H27, H3, H29). */
export const Agents: Part = (home) => ({
    extension: defineExtension({
        name: "hamlet.agents",
        tools: [
            defineMayorTool(home, {
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
                change: async (tx, args, { place, limits, task }) => {
                    const admitted = await admit(tx, place.hamlet, {
                        ...args,
                        by: "mayor",
                        at: home.now(),
                        task,
                    });
                    if (!admitted.ok) return admitted;
                    const name = tidyName(args.name);
                    const allowed =
                        !args.model || limits.models.includes(args.model);
                    const words = [
                        `Created \`${name}\`. Give it work with ask_agent.`,
                    ];
                    if (!allowed)
                        words.push(
                            `This hamlet may not run \`${args.model}\`; the agent runs the hamlet's own model.`,
                        );
                    for (const road of admitted.value.roadRefusals ?? [])
                        words.push(
                            `Road to \`${road.peer}\` refused: ${recordRefusalText(road.refusal, "agent")}.`,
                        );
                    return ok(words.join("\n"));
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
                    if (!agent) return;
                    const built = await home.houses
                        .of({ hamlet: place.hamlet, number: agent.number })
                        .build(context);
                    // The agent is recorded; its house reports its own state until a later open prepares it again.
                    if (!built.ok)
                        console.error(
                            `Hamlet could not prepare the workspace house of agent ${agent.number} in ${place.hamlet}: ${home.secrets.words(built.error.message)}`,
                        );
                },
            }),
            defineMayorTool(home, {
                name: "build_road",
                description:
                    "Build a road between two agents, charged to the hamlet.",
                parameters: Type.Object({
                    a: Type.String(),
                    b: Type.String(),
                    reason: Type.String(),
                }),
                change: async (tx, args, { place, task }) => {
                    const built = await layRoad(
                        tx,
                        place.hamlet,
                        args.a,
                        args.b,
                        {
                            by: "mayor",
                            reason: args.reason,
                            at: home.now(),
                            task,
                        },
                    );
                    return built.ok
                        ? ok(
                              `Built a road between \`${args.a}\` and \`${args.b}\`.`,
                          )
                        : built;
                },
            }),
            defineMayorTool(home, {
                name: "set_model",
                description:
                    "Choose an allowed model for an agent, or empty for the hamlet's own.",
                parameters: Type.Object({
                    name: Type.String(),
                    model: Type.String(),
                    reason: Type.String(),
                }),
                change: async (tx, args, { place, limits }) => {
                    const record = await tx.doc(
                        RecordDoc,
                        place.hamlet,
                        place.hamlet,
                    );
                    const agent = named(record, args.name);
                    if (!agent || agent.number === record.mayor)
                        return recordRefusal("set-model", place.hamlet, {
                            kind: "agent-missing",
                            name: args.name,
                        });
                    if (Object.hasOwn(limits.pinned, agent.name))
                        return recordRefusal("set-model", place.hamlet, {
                            kind: "model-pinned",
                            name: agent.name,
                        });
                    if (args.model && !limits.models.includes(args.model))
                        return recordRefusal("set-model", place.hamlet, {
                            kind: "model-not-allowed",
                            model: args.model,
                            models: [...limits.models],
                        });
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
                    return ok(
                        `\`${agent.name}\` now runs ${args.model || "the hamlet's own model"}.`,
                    );
                },
            }),
        ],
    }),
});

/** A chief's road building deed. */
export const road: RecordDeed = async (home, given, context) => {
    const [a, b, ...reason] = given.rest.trim().split(/\s+/);
    if (!a || !b)
        return recordRefusal("road", given.hamlet, {
            kind: "record-usage",
            command: "road",
        });
    await reconcile(home, context);
    return home.harness.commit(async (tx) => {
        const result = await layRoad(tx, given.hamlet, a, b, {
            by: `chief:${given.chief}`,
            reason: reason.join(" "),
            at: home.now(),
        });
        return result.ok
            ? ok(`Built a road between \`${a}\` and \`${b}\`.`)
            : result;
    }, context);
};
/** A chief's road removal deed. */
export const unroad: RecordDeed = async (home, given, context) => {
    const [a, b, ...reason] = given.rest.trim().split(/\s+/);
    if (!a || !b)
        return recordRefusal("unroad", given.hamlet, {
            kind: "record-usage",
            command: "unroad",
        });
    return home.harness.commit(async (tx) => {
        const result = await takeUpRoad(tx, given.hamlet, a, b, {
            by: `chief:${given.chief}`,
            reason: reason.join(" "),
            at: home.now(),
        });
        return result.ok
            ? ok(
                  `Took up the road between \`${a}\` and \`${b}\`. Nothing is refunded.`,
              )
            : result;
    }, context);
};
