import type { Context } from "@earendil-works/chord";
import { Type } from "typebox";
import { Value } from "typebox/value";
import { commitEdit } from "../chats/configure.ts";
import type { Home } from "../home.ts";
import { type ChiefRefusal, chiefAccess } from "../realm/access.ts";
import { err, ok, type Result } from "../result.ts";
import { LimitsDoc, type LimitsRefusal, parseLimits } from "./rules.ts";

type LimitsEdit = (
    | { whole: unknown }
    | { set: { section: string; key: string; value: unknown } }
) & { reason?: string };
const Section = Type.Record(Type.String(), Type.Unknown());

/** Commits valid limits together with every model choice the edit changes. */
export async function editLimits(
    home: Home,
    hamlet: string,
    edit: LimitsEdit,
    context: Context,
): Promise<Result<void, LimitsRefusal | ChiefRefusal["error"]>> {
    const access = await chiefAccess(home, hamlet, "edit limits", context);
    if (!access.ok) return access;
    return home.harness.commit(async (tx) => {
        const stored = await tx.doc(LimitsDoc, hamlet, hamlet);
        const facts = {
            operation: "edit-limits",
            hamlet,
            retrySafe: true,
        } as const;
        let parsed: ReturnType<typeof parseLimits>;
        if ("whole" in edit) parsed = parseLimits(edit.whole);
        else {
            const { section, key, value } = edit.set;
            const fields = Object.entries(stored).find(
                ([name]) => name === section,
            )?.[1];
            if (!Value.Check(Section, fields))
                return err({
                    ...facts,
                    kind: "unknown-field",
                    path: section,
                    field: section,
                    parent: "limits",
                    scope: "section",
                } as const);
            const reset =
                value === null &&
                !(
                    (section === "budget" || section === "task") &&
                    ["cost", "minutes", "tokens"].includes(key)
                );
            if (reset && !Object.hasOwn(fields, key) && section !== "pinned")
                return err({
                    ...facts,
                    kind: "unknown-field",
                    path: `${section}.${key}`,
                    field: key,
                    parent: section,
                    scope: "field",
                } as const);
            const replacement: readonly (readonly [string, unknown])[] = reset
                ? []
                : [[key, value]];
            const changed = Object.fromEntries([
                ...Object.entries(fields).filter(([name]) => name !== key),
                ...replacement,
            ]);
            parsed = parseLimits({ ...stored, [section]: changed });
        }
        if (!parsed.ok) return err({ ...parsed.error, ...facts });
        const next = parsed.value;
        await commitEdit(home, tx, {
            by: home.chief,
            ...(edit.reason === undefined ? {} : { reason: edit.reason }),
            change: () => {
                Object.assign(stored, next);
            },
        });
        return ok(undefined);
    }, context);
}
