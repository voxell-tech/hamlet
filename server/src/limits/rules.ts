import { defineDocFamily } from "@earendil-works/pi-durable";
import { type Static, type TProperties, Type } from "typebox";
import { Value } from "typebox/value";
import { err, ok, type Result } from "../result.ts";

function section<P extends TProperties>(properties: P) {
    return Type.Object(properties, {
        additionalProperties: false,
        default: {},
    });
}
function whole(value: number) {
    return Type.Integer({
        minimum: 0,
        maximum: Number.MAX_SAFE_INTEGER,
        default: value,
    });
}
function cost(value: number) {
    return Type.Number({ minimum: 0, default: value });
}
function cap(value: number | null, money = false) {
    return Type.Union(
        [
            money
                ? Type.Number({ minimum: 0 })
                : Type.Integer({
                      minimum: 0,
                      maximum: Number.MAX_SAFE_INTEGER,
                  }),
            Type.Null(),
        ],
        { default: value },
    );
}
function flag(value: boolean) {
    return Type.Boolean({ default: value });
}
function names() {
    return Type.Array(Type.String(), { default: [] });
}

const Pinned = Type.Record(Type.String(), Type.String(), { default: {} });

/** A hamlet's limits and their omitted-field defaults (H22). Dollar caps may be fractional; counts and minutes are whole. */
export const LimitsSchema = Type.Object(
    {
        votes: section({
            passYes: whole(2),
            passOf: whole(3),
            leastVoters: whole(3),
            sealed: flag(true),
            groundedOnly: flag(false),
            ballotMinutes: whole(5),
            openMinutes: whole(60),
            retryMinutes: whole(60),
        }),
        hamlets: section({
            most: whole(6),
            roadsBetween: flag(true),
            removalMayors: whole(3),
        }),
        budget: section({
            period: Type.Union([Type.Literal("day"), Type.Literal("month")], {
                default: "day",
            }),
            cost: cap(5, true),
            minutes: cap(null),
            tokens: cap(null),
        }),
        task: section({
            messages: whole(12),
            cost: cap(1, true),
            minutes: cap(null),
            tokens: cap(null),
        }),
        road: section({
            tokens: whole(0),
            cost: cost(0.01),
            minutes: whole(0),
            growsOverDays: whole(7),
        }),
        mostAgents: whole(12),
        mostRoads: whole(24),
        models: names(),
        model: Type.Union([Type.String(), Type.Null()], { default: null }),
        pinned: Pinned,
        mayor: section({
            createAgents: flag(true),
            buildRoads: flag(true),
            removeAgents: flag(true),
            mergeAgents: flag(true),
            toldDays: whole(7),
        }),
        leaving: section({
            protectedMinutes: whole(0),
            mergedNotes: whole(6000),
        }),
        asking: section({
            mayorReview: Type.Union(
                [
                    Type.Literal("always"),
                    Type.Literal("never"),
                    Type.Literal("remote"),
                ],
                { default: "remote" },
            ),
            refusals: whole(3),
            questions: whole(3),
        }),
        permissions: section({ network: names(), unconfined: names() }),
    },
    { additionalProperties: false },
);
/** A chief's limits; a cap of zero stops its meter and null leaves it uncapped. */
export type Limits = Static<typeof LimitsSchema>;

type Constraint =
    | "expected a section"
    | "expected model names by agent name"
    | "expected a list of names"
    | "a cost is a number of zero or more"
    | "wrong type or value";
/** Invalid limits are refused before a hamlet's documents or model choices change. */
export type LimitsRefusal = {
    operation: "parse-limits" | "edit-limits";
    retrySafe: true;
    hamlet?: string;
} & (
    | { kind: "invalid-field"; path: string; constraint: Constraint }
    | {
          kind: "unknown-field";
          path: string;
          field: string;
          parent: string;
          scope: "field" | "section";
      }
    | { kind: "votes-inconsistent"; reason: "pass-of-zero" | "yes-exceeds-of" }
);
/** The chief's existing field and threshold wording. */
export function limitsRefusalText(error: LimitsRefusal): string {
    switch (error.kind) {
        case "invalid-field":
            return `${error.path}: ${error.constraint}`;
        case "unknown-field":
            return error.scope === "section"
                ? `unknown section \`${error.field}\``
                : `unknown field \`${error.field}\` under ${error.parent}`;
        case "votes-inconsistent":
            return error.reason === "pass-of-zero"
                ? "votes.passOf: must be at least 1"
                : "votes.passYes: cannot exceed votes.passOf";
    }
    const unhandled: never = error;
    throw new Error(`Unclassified limits refusal: ${unhandled}`);
}

function pointer(path: string): string[] {
    return path
        .split("/")
        .slice(1)
        .map((part) => part.replaceAll("~1", "/").replaceAll("~0", "~"));
}
const ObjectInput = Type.Record(Type.String(), Type.Unknown());
/** Applies only omitted defaults and returns independent validated limits. */
export function parseLimits(input: unknown): Result<Limits, LimitsRefusal> {
    const facts = { operation: "parse-limits", retrySafe: true } as const;
    if (!Value.Check(ObjectInput, input))
        return err({
            ...facts,
            kind: "invalid-field",
            path: "limits",
            constraint: "expected a section",
        });
    const errors = Value.Errors(LimitsSchema, input).filter(
        (error) => error.keyword !== "required",
    );
    if (errors.length > 0) {
        const extra = errors.find(
            (error) => error.keyword === "additionalProperties",
        );
        if (extra !== undefined) {
            const parent = pointer(extra.instancePath);
            const fields = extra.params.additionalProperties;
            const field = Array.isArray(fields) ? fields[0] : undefined;
            if (typeof field !== "string")
                throw new Error(
                    "The limits validator omitted its unknown field.",
                );
            return err({
                ...facts,
                kind: "unknown-field",
                path: [...parent, field].join("."),
                field,
                parent: parent.join(".") || "limits",
                scope: "field",
            });
        }
        const error = errors[0];
        if (error === undefined)
            throw new Error(
                "The limits validator omitted its failed constraint.",
            );
        let parts = pointer(error.instancePath);
        if (parts[0] === "pinned")
            return err({
                ...facts,
                kind: "invalid-field",
                path: "pinned",
                constraint: "expected model names by agent name",
            });
        if (error.schemaPath.includes("/items")) parts = parts.slice(0, -1);
        const path = parts.join(".") || "limits";
        const constraint: Constraint = path.endsWith(".cost")
            ? "a cost is a number of zero or more"
            : error.schemaPath.includes("/items") ||
                (error.keyword === "type" && error.params.type === "array")
              ? "expected a list of names"
              : error.keyword === "type" && error.params.type === "object"
                ? "expected a section"
                : "wrong type or value";
        return err({ ...facts, kind: "invalid-field", path, constraint });
    }
    const copy = Value.Clone(input);
    // TypeBox cloning omits special keys; pinned names are data and must survive.
    if (Value.Check(Pinned, input.pinned)) copy.pinned = { ...input.pinned };
    const value = Value.Default(LimitsSchema, copy);
    if (!Value.Check(LimitsSchema, value))
        throw new Error("Default limits do not satisfy their schema.");
    if (value.votes.passOf < 1)
        return err({
            ...facts,
            kind: "votes-inconsistent",
            reason: "pass-of-zero",
        });
    if (value.votes.passYes > value.votes.passOf)
        return err({
            ...facts,
            kind: "votes-inconsistent",
            reason: "yes-exceeds-of",
        });
    return ok(value);
}

/** Fresh defaults are derived from the same schema that accepts a chief's edits. */
export function defaultLimits(): Limits {
    const parsed = parseLimits({});
    if (!parsed.ok)
        throw new Error(
            `Invalid default limits: ${limitsRefusalText(parsed.error)}`,
        );
    return parsed.value;
}

/** Each hamlet's limits, retaining the stored kind and migration version. */
export const LimitsDoc = defineDocFamily<Limits, string>({
    kind: "hamlet.limits",
    version: 2,
    migrate: (value) => {
        const parsed = parseLimits(value);
        if (!parsed.ok)
            throw new Error(
                `Invalid stored limits: ${limitsRefusalText(parsed.error)}`,
            );
        return parsed.value;
    },
    scope: "session",
    family: true,
    initial: defaultLimits,
});
/** Stored limits, or fresh defaults for a hamlet without a document. */
export function limitsOf(
    stored: Readonly<Limits> | undefined,
): Readonly<Limits> {
    return stored ?? defaultLimits();
}
