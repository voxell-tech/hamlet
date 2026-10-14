import { defineDocFamily } from "@earendil-works/pi-durable";

/** Caps on spend; `null` leaves a meter uncapped. */
type Caps = {
    /** Dollars. */
    cost: number | null;
    /** Minutes of model time. */
    minutes: number | null;
    /** Counted and never capped (H8). */
    tokens: number | null;
};

/** A chief's limits on one hamlet (H22). A cap of 0 stops everything. */
export type Limits = {
    /** The hamlet's budget for a period (H8). */
    budget: Caps & { period: "day" | "month" };
    /** The caps on one task, across every agent it reaches (H12). */
    task: Caps & {
        /** Messages between agents. */
        messages: number;
    };
    /** The price of one road (H3). */
    road: {
        tokens: number;
        cost: number;
        minutes: number;
        /** Days unused after which a road is shown grown over. */
        growsOverDays: number;
    };
    mostAgents: number;
    mostRoads: number;
    /** Models the mayor may choose besides the hamlet's own, as `endpoint::id`. */
    models: string[];
    /** The hamlet's own model; `null` is the home's default (H29). */
    model: string | null;
    /** The chief's choice of model by agent name, `mayor` for the office (H29). */
    pinned: { [name: string]: string };
    /** The acts the mayor may do without asking (H22). */
    mayor: {
        createAgents: boolean;
        buildRoads: boolean;
        removeAgents: boolean;
        mergeAgents: boolean;
        /** Days of the mayor's decisions its agents are told (H17). */
        toldDays: number;
    };
    /** Agents leaving the hamlet (H5, H7). */
    leaving: {
        protectedMinutes: number;
        mergedNotes: number;
    };
    /** The most agents may ask of a chief (H10, H11). */
    asking: {
        mayorReview: "always" | "never" | "remote";
        /** Refusals in one turn after which further requests are refused without asking. */
        refusals: number;
        /** Questions one turn may put to a chief. */
        questions: number;
    };
    /** Agents that reach past the sandbox without asking, by name (H11). */
    permissions: {
        network: string[];
        unconfined: string[];
    };
};

/** The limits a hamlet has before a chief changes them. */
export function defaultLimits(): Limits {
    return {
        budget: { period: "day", cost: 5, minutes: null, tokens: null },
        task: { messages: 12, cost: 1, minutes: null, tokens: null },
        road: { tokens: 0, cost: 0.01, minutes: 0, growsOverDays: 7 },
        mostAgents: 12,
        mostRoads: 24,
        models: [],
        model: null,
        pinned: {},
        mayor: {
            createAgents: true,
            buildRoads: true,
            removeAgents: true,
            mergeAgents: true,
            toldDays: 7,
        },
        leaving: { protectedMinutes: 0, mergedNotes: 6000 },
        asking: { mayorReview: "remote", refusals: 3, questions: 3 },
        permissions: { network: [], unconfined: [] },
    };
}

/** Each hamlet's limits, by hamlet id. */
export const LimitsDoc = defineDocFamily<Limits, string>({
    kind: "hamlet.limits",
    version: 1,
    scope: "session",
    family: true,
    initial: defaultLimits,
});

/** The limits a stored document holds, the defaults where there is none. */
export function limitsOf(
    stored: Readonly<Limits> | undefined,
): Readonly<Limits> {
    return stored ?? defaultLimits();
}
