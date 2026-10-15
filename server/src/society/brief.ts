import { defineExtension, section } from "@earendil-works/pi-durable";
import { agentOf } from "../errands.ts";
import { agentKey } from "../hamlet.ts";
import { InstructionsDoc } from "../instructions.ts";
import { LimitsDoc, limitsOf } from "../limits.ts";
import type { Part } from "../parts.ts";
import { liveHamlets, RealmDoc, realmRoads, roadsOf } from "../realm.ts";
import {
    addressOf,
    type Decision,
    type End,
    type HamletRecord,
    RecordDoc,
} from "../record.ts";

/** The hamlet's prompt section (C11, H17). */
export const HamletBrief: Part = (home) => ({
    extension: defineExtension({
        name: "hamlet.brief",
        sections: [
            section(
                "hamlet",
                async (input, context) => {
                    const place = await agentOf(
                        input.read,
                        input.conversationId,
                        context,
                    );
                    if (!place) return undefined;
                    const record = await input.read.snapshot(
                        RecordDoc,
                        place.hamlet,
                        context,
                    );
                    const agent = record?.agents.find(
                        (a) => a.number === place.number,
                    );
                    if (!record || !agent) return undefined;
                    const limits = limitsOf(
                        await input.read.snapshot(
                            LimitsDoc,
                            place.hamlet,
                            context,
                        ),
                    );
                    const votes = `\nAny agent may call a vote with \`call_vote\`, to replace the mayor or to found a new hamlet under a new mayor. Every agent then votes from its own house, it passes with ${limits.votes.passYes} of ${limits.votes.passOf} of the voters, and the chief is told and may overturn it. Call one only on grounds from your own work, not on another agent's say-so.\n`;
                    if (record.mayor === place.number) {
                        const lines = [
                            "# Your hamlet",
                            "You are the mayor of this hamlet. `create_agent` adds an agent with its own house (its own notes, skills and memory, separate from yours), `ask_agent` gives one work and returns its reply, and `build_road` lets two agents talk to each other directly. An agent without a road to another can only reach it through you. Give `create_agent` a `character` to say how the new agent behaves. Create an agent when a kind of work will come back and deserves its own memory; do small one-off things yourself. Before creating one, check the list below for an agent that already fits. `merge_agents` makes one agent of two whose work overlaps, and `remove_agent` retires one nobody needs. Both keep everything in the archive; neither may be used on an agent that has a task from a chief. `set_model` chooses an allowed model for an agent. Each house is closed to other agents; ask an agent for what you need.",
                        ];
                        if (place.number !== 0)
                            lines.push(
                                `You are \`${agent.name}\`, and you were made mayor in place of another. Your work before was: ${agent.purpose} The former mayors are ordinary agents now and are in the list.`,
                            );
                        lines.push(
                            "Agents:",
                            ...members(record, place.number),
                            ...farewells(record),
                        );
                        const instructions = [];
                        for (const a of record.agents.filter(
                            (a) => a.number !== place.number,
                        )) {
                            const doc = await input.read.snapshot(
                                InstructionsDoc,
                                agentKey({
                                    hamlet: place.hamlet,
                                    number: a.number,
                                }),
                                context,
                            );
                            for (const i of doc?.given ?? [])
                                instructions.push({ ...i, name: a.name });
                        }
                        const latest = instructions
                            .sort((a, b) => a.at - b.at)
                            .slice(-5);
                        if (latest.length) {
                            lines.push(
                                "A chief can instruct your agents directly, without asking you. You may tell the chief if you think one of these was a mistake, but you may not undo it or act against the agent for following it. The latest:",
                            );
                            lines.push(
                                ...latest.map(
                                    (i) =>
                                        `- ${i.chief} told ${i.name}: ${clip(i.text)}`,
                                ),
                            );
                        }
                        return lines.join("\n") + votes;
                    }
                    const roads = roadsOf(record).flatMap((r) =>
                        r.a === place.number
                            ? [endName(record, r.b)]
                            : r.b === place.number
                              ? [endName(record, r.a)]
                              : [],
                    );
                    const realm = await input.read.snapshot(RealmDoc, context);
                    if (realm) {
                        const records: Record<
                            string,
                            Readonly<HamletRecord>
                        > = { [place.hamlet]: record };
                        for (const h of liveHamlets(realm)) {
                            const value = await input.read.snapshot(
                                RecordDoc,
                                h.id,
                                context,
                            );
                            if (value) records[h.id] = value;
                        }
                        for (const road of realmRoads(realm, records)) {
                            const peer =
                                road.a.hamlet === place.hamlet &&
                                road.a.number === place.number
                                    ? road.b
                                    : road.b.hamlet === place.hamlet &&
                                        road.b.number === place.number
                                      ? road.a
                                      : undefined;
                            const there = peer && records[peer.hamlet];
                            if (peer && there)
                                roads.push(
                                    `${addressOf(there, peer.number)}@${peer.hamlet}`,
                                );
                        }
                    }
                    return [
                        "# Your place",
                        `You are \`${agent.name}\`, an agent in a hamlet. Your purpose: ${agent.purpose}`,
                        "Messages come from the mayor or from a chief, a human the hamlet belongs to. Each agent's house is closed to other agents: put what they need in your reply. A chief's instruction outranks the mayor's. If you think a chief's instruction is a mistake, use `object` once and wait for the reply before doing anything. If the chief says to go ahead, do it and do not object again. You may not refuse.",
                        `You can talk only along your roads, with \`ask_agent\` or \`tell_agent\`. Your roads lead to: ${roads.join(", ")}.`,
                        "The other agents of the hamlet:",
                        ...members(record, place.number),
                        ...farewells(record, place.number),
                        votes,
                        standing(
                            record,
                            place.number,
                            home.now(),
                            limits.mayor.toldDays,
                        ),
                    ].join("\n");
                },
                { tag: false },
            ),
        ],
    }),
});

function members(record: Readonly<HamletRecord>, except: number): string[] {
    const lines = record.agents
        .filter((a) => a.number !== except && a.number !== record.mayor)
        .map((a) => `- ${a.name}: ${a.purpose}`);
    return lines.length ? lines : ["(none yet)"];
}
function farewells(record: Readonly<HamletRecord>, reader?: number): string[] {
    const gone = record.archive
        .filter(
            (a) =>
                a.back === null &&
                !record.agents.some((live) => live.name === a.agent.name),
        )
        .slice(-5);
    if (!gone.length) return [];
    return [
        "No longer here, and not to be asked or told:",
        ...gone.map((a) => {
            if (a.how === "removed") return `- ${a.agent.name}: removed`;
            if (a.into === reader)
                return `- ${a.agent.name}: merged into you; its notes, skills and roads are yours`;
            const kept =
                record.agents.find((agent) => agent.number === a.into)?.name ??
                a.merge?.kept.name ??
                "the kept agent";
            return `- ${a.agent.name}: merged into ${kept}; talk to ${kept}`;
        }),
    ];
}
function clip(text: string) {
    return [...text].slice(0, 160).join("");
}
function agentName(record: Readonly<HamletRecord>, number: number): string {
    return (
        record.agents.find((a) => a.number === number)?.name ??
        record.archive.findLast((a) => a.agent.number === number)?.agent.name ??
        `agent #${number}`
    );
}
function endName(record: Readonly<HamletRecord>, end: End) {
    return end === "office" ? "mayor" : agentName(record, end);
}
function standing(
    record: Readonly<HamletRecord>,
    reader: number,
    now: number,
    days: number,
): string {
    const since = Math.max(0, now - days * 86400000);
    const counts = new Map<string, number>();
    const decided: string[] = [],
        mine: string[] = [];
    for (const d of [...record.decisions].reverse()) {
        if (d.at < since || (counts.get(d.kind) ?? 0) >= 6) continue;
        if (d.kind === "road_refused" && d.from !== reader) continue;
        const by =
            "by" in d
                ? d.by === "mayor"
                    ? "The mayor"
                    : d.by.startsWith("vote:")
                      ? "A vote"
                      : "A chief"
                : "A chief";
        const reason = "reason" in d ? clip(d.reason) || "no reason given" : "";
        const words = decisionWords(record, d, by);
        if (!words) continue;
        counts.set(d.kind, (counts.get(d.kind) ?? 0) + 1);
        (d.kind === "road_refused" ? mine : decided).push(
            `- ${words}: ${reason}`,
        );
    }
    if (!mine.length && !decided.length) return "";
    return [
        "# How your hamlet was run lately",
        "From the hamlet's record, each with the reason that was given. These are what happened, not a judgement.",
        ...(mine.length ? ["What touched you:", ...mine] : []),
        ...(decided.length
            ? ["What was decided, latest first:", ...decided]
            : []),
    ].join("\n");
}
function decisionWords(
    record: Readonly<HamletRecord>,
    d: Decision,
    by: string,
): string | undefined {
    switch (d.kind) {
        case "created":
            return `${by} created \`${d.name}\``;
        case "removed":
        case "restored":
        case "erased":
            return `${by} ${d.kind} \`${d.name}\``;
        case "merged":
            return `${by} merged \`${agentName(record, d.absorb)}\` into \`${agentName(record, d.keep)}\``;
        case "road_built":
        case "road_removed":
            return `${by} ${d.kind === "road_built" ? "built" : "took up"} a road between \`${endName(record, d.a)}\` and \`${endName(record, d.b)}\``;
        case "road_refused":
            return `The mayor did not build the road you asked for to \`${d.to}\``;
        case "mayor_changed":
            return `${by} made \`${agentName(record, d.to)}\` mayor in place of \`${agentName(record, d.from)}\``;
        case "model":
            return `${by} chose ${d.model || "the hamlet's own model"} for \`${agentName(record, d.agent)}\``;
        case "moved_in":
        case "moved_out":
            return `${by} moved \`${d.name}\` ${d.kind === "moved_in" ? "from" : "to"} \`${d.hamlet}\``;
        default:
            return undefined;
    }
}
