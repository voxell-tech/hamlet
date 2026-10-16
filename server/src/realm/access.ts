import type { Context } from "@earendil-works/chord";
import type { Home } from "../home.ts";
import { isChief, RealmDoc } from "./places.ts";

/** A refused chief operation, safe to repeat after the named recovery. */
export type ChiefRefusal = {
    ok: false;
    error: {
        operation: string;
        hamlet: string;
        retrySafe: true;
    } & (
        | { kind: "hamlet-missing"; recovery: "choose-hamlet" }
        | { kind: "hamlet-removed"; reason: string; recovery: "restore-hamlet" }
        | { kind: "not-chief"; recovery: "ask-chief" }
    );
};

/** An admitted chief operation or its classified refusal. */
export type ChiefResult<T> = { ok: true; value: T } | ChiefRefusal;

/** Requires a living hamlet led by the chief, defaulting to the connected chief. */
export async function chiefAccess(
    home: Home,
    hamlet: string,
    operation: string,
    context: Context,
    chief = home.chief,
): Promise<ChiefResult<null>> {
    const realm = await home.harness.snapshot(RealmDoc, context);
    const place = realm?.hamlets.find((h) => h.id === hamlet);
    const facts = { operation, hamlet, retrySafe: true } as const;
    if (!realm || !place || place.erased)
        return {
            ok: false,
            error: {
                ...facts,
                kind: "hamlet-missing",
                recovery: "choose-hamlet",
            },
        };
    if (place.removed)
        return {
            ok: false,
            error: {
                ...facts,
                kind: "hamlet-removed",
                reason: place.removed.reason,
                recovery: "restore-hamlet",
            },
        };
    if (!isChief(realm, hamlet, chief))
        return {
            ok: false,
            error: { ...facts, kind: "not-chief", recovery: "ask-chief" },
        };
    return { ok: true, value: null };
}

/** Chief-facing recovery for an access refusal; request correlation belongs to the envelope. */
export function chiefRefusalText({ error }: ChiefRefusal): string {
    switch (error.kind) {
        case "hamlet-missing":
            return `Cannot ${error.operation}: no hamlet named \`${error.hamlet}\`. Choose a living hamlet.`;
        case "hamlet-removed":
            return `Cannot ${error.operation}: Hamlet \`${error.hamlet}\` was removed: ${error.reason}. Ask a chief to restore it first.`;
        case "not-chief":
            return `Cannot ${error.operation}: Only a chief of that hamlet may reach its agents. Ask one of its chiefs.`;
    }
    const unhandled: never = error;
    throw new Error(`Unclassified chief access failure: ${unhandled}`);
}
