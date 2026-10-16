import { ok, type Result } from "../result.ts";
import type { AgentRecord, Archived, End, HamletRecord } from "./document.ts";
import { FOUNDER, MAYOR } from "./document.ts";
import { type RecordRefusal, refusal } from "./refusals.ts";
/** The agent `agent` names in `record`: `mayor` for whoever holds the office, otherwise by name. */
export function named(
    record: Readonly<HamletRecord>,
    agent: string,
): AgentRecord | undefined {
    const number = agent === MAYOR ? record.mayor : undefined;
    return record.agents.find((each) =>
        number === undefined ? each.name === agent : each.number === number,
    );
}

/** The address of agent `number` in `record`: `mayor` while it holds the office, otherwise its name. */
export function addressOf(
    record: Readonly<HamletRecord>,
    number: number,
): string {
    if (record.mayor === number) return MAYOR;
    return record.agents.find((each) => each.number === number)?.name ?? "";
}

/** An agent name normalized from a model's spelling (C10). */
export function tidyName(name: string): string {
    let tidy = "";
    for (const c of name.trim()) {
        if (/[A-Z]/.test(c) && /[a-z]$/.test(tidy)) tidy += "-";
        if (c === " " || c === "_" || c === "-") {
            if (!tidy.endsWith("-")) tidy += "-";
        } else tidy += c.replace(/[A-Z]/g, (c) => c.toLowerCase());
    }
    return tidy.replace(/^-+|-+$/g, "");
}

/** Validates the name alphabet and reserved addresses (C10). */
export function checkName(
    name: string,
    hamlet = "",
    operation = "name",
): Result<void, RecordRefusal> {
    if (!/^[a-z-]{1,24}$/.test(name))
        return refusal(operation, hamlet, { kind: "name-format" });
    if ([MAYOR, FOUNDER, "all"].includes(name))
        return refusal(operation, hamlet, { kind: "name-reserved", name });
    return ok(undefined);
}

type NameFallback = "recorded" | "created" | "agent-number";

/** Living and archived names take precedence over the selected historical fallback. */
export function nameOf(
    record: Readonly<HamletRecord> | undefined,
    number: number,
    fallback: NameFallback = "recorded",
): string {
    const known =
        record?.agents.find((a) => a.number === number)?.name ??
        record?.archive.findLast((a) => a.agent.number === number)?.agent.name;
    if (known !== undefined) return known;
    switch (fallback) {
        case "agent-number":
            return `agent #${number}`;
        case "created":
            return (
                record?.decisions
                    .filter((d) => d.kind === "created")
                    .find((d) => d.agent === number)?.name ?? String(number)
            );
        case "recorded":
            return (
                record?.decisions
                    .flatMap((d) =>
                        "name" in d && "agent" in d
                            ? [{ number: d.agent, name: d.name }]
                            : [],
                    )
                    .findLast((d) => d.number === number)?.name ??
                String(number)
            );
    }
    const unhandled: never = fallback;
    throw new Error(`Unclassified name fallback: ${unhandled}`);
}
/** A road end in words, preserving the office address. */
export function endName(
    record: Readonly<HamletRecord>,
    end: End,
    fallback: NameFallback = "recorded",
): string {
    return end === "office" ? "mayor" : nameOf(record, end, fallback);
}
/** The recorded author in words for a chief. */
export function byWords(by: string): string {
    return by === "mayor"
        ? "the mayor"
        : by.startsWith("chief:")
          ? `the chief ${by.slice(6)}`
          : by;
}
/** A departed agent's refusal (H28). */
export function farewell(
    entry: Archived,
    record: Readonly<HamletRecord>,
): string {
    const name = entry.agent.name;
    const by =
        entry.by === MAYOR
            ? "the mayor"
            : entry.by.startsWith("chief:")
              ? `the chief ${entry.by.slice(6)}`
              : entry.by;
    if (entry.how === "removed")
        return `\`${name}\` was removed from the hamlet by ${by} ("${entry.reason}"). Its house is archived and its roads went with it; only a chief can restore it.`;
    const kept =
        record.agents.find((a) => a.number === entry.into)?.name ??
        entry.merge?.kept.name ??
        "the kept agent";
    return `\`${name}\` was merged into \`${kept}\` by ${by} ("${entry.reason}"). \`${kept}\` has its notes, skills and roads; talk to \`${kept}\`.`;
}
