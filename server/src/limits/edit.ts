import type { Context } from "@earendil-works/chord";
import { commitEdit } from "../chats/configure.ts";
import type { Home } from "../home.ts";
import { defaultLimits, type Limits, LimitsDoc } from "./rules.ts";

type LimitsEdit = (
    | { whole: unknown }
    | { set: { section: string; key: string; value: unknown } }
) & { reason?: string };

/** A validated limits document with defaults for omitted fields. */
export function validateLimits(value: unknown): Limits {
    const merged = merge(defaultLimits(), value, "") as Limits;
    if (merged.votes.passOf < 1)
        throw new Error("votes.passOf: must be at least 1");
    if (merged.votes.passYes > merged.votes.passOf)
        throw new Error("votes.passYes: cannot exceed votes.passOf");
    return merged;
}

function object(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}
function merge(defaults: unknown, given: unknown, path: string): unknown {
    if (path === "pinned") {
        if (
            !object(given) ||
            Object.values(given).some((v) => typeof v !== "string")
        )
            throw new Error("pinned: expected model names by agent name");
        return { ...given };
    }
    if (object(defaults)) {
        if (!object(given))
            throw new Error(`${path || "limits"}: expected a section`);
        const result = { ...defaults };
        for (const [key, value] of Object.entries(given)) {
            if (!Object.hasOwn(defaults, key))
                throw new Error(
                    `unknown field \`${key}\` under ${path || "limits"}`,
                );
            result[key] = merge(
                defaults[key],
                value,
                path ? `${path}.${key}` : key,
            );
        }
        return result;
    }
    if (Array.isArray(defaults)) {
        if (!Array.isArray(given) || given.some((v) => typeof v !== "string"))
            throw new Error(`${path}: expected a list of names`);
        return [...given];
    }
    if (path.endsWith(".cost")) {
        if (given === null && path !== "road.cost") return null;
        if (typeof given !== "number" || !Number.isFinite(given) || given < 0)
            throw new Error(`${path}: a cost is a number of zero or more`);
        return given;
    }
    if (path === "model") {
        if (given === null || typeof given === "string") return given;
    } else if (path === "budget.period") {
        if (given === "day" || given === "month") return given;
    } else if (path === "asking.mayorReview") {
        if (given === "always" || given === "never" || given === "remote")
            return given;
    } else if (defaults === null || typeof defaults === "number") {
        if (given === null && defaults === null) return null;
        if (
            typeof given === "number" &&
            Number.isSafeInteger(given) &&
            given >= 0
        )
            return given;
    } else if (typeof given === typeof defaults) return given;
    throw new Error(`${path}: wrong type or value`);
}

/** A chief's atomic edit, including any model choice it changes (H22, H29). */
export async function editLimits(
    home: Home,
    hamlet: string,
    edit: LimitsEdit,
    context: Context,
): Promise<void> {
    await home.harness.commit(
        (tx) =>
            commitEdit(home, tx, {
                by: home.chief,
                ...(edit.reason === undefined ? {} : { reason: edit.reason }),
                change: async (tx) => {
                    const stored = await tx.doc(LimitsDoc, hamlet, hamlet);
                    let next: unknown;
                    if ("whole" in edit) next = edit.whole;
                    else {
                        const { section, key, value } = edit.set;
                        const copy = JSON.parse(
                            JSON.stringify(stored),
                        ) as Record<string, unknown>;
                        if (
                            !Object.hasOwn(copy, section) ||
                            !object(copy[section])
                        )
                            throw new Error(`unknown section \`${section}\``);
                        const fields = { ...copy[section] };
                        if (
                            value === null &&
                            !(
                                (section === "budget" || section === "task") &&
                                ["cost", "minutes", "tokens"].includes(key)
                            )
                        ) {
                            if (
                                !Object.hasOwn(fields, key) &&
                                section !== "pinned"
                            )
                                throw new Error(
                                    `unknown field \`${key}\` under ${section}`,
                                );
                            delete fields[key];
                        } else fields[key] = value;
                        copy[section] = fields;
                        next = copy;
                    }
                    Object.assign(stored, validateLimits(next));
                },
            }),
        context,
    );
}
