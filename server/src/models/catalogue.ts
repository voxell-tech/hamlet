import type { Context } from "@earendil-works/chord";
import { defineDoc } from "@earendil-works/pi-durable";
import { type Static, Type } from "typebox";
import type { Home } from "../home.ts";
import { err, ok, type Result } from "../result.ts";
import { assumedRate, isLocal } from "./endpoints.ts";
import type { CatalogueFailure } from "./failure.ts";

/** Input and output costs per million tokens. */
export const PriceSchema = Type.Object({
    input: Type.Number({ minimum: 0 }),
    output: Type.Number({ minimum: 0 }),
});
/** Input and output cost in dollars per million tokens. */
export type Price = Static<typeof PriceSchema>;
const CardSchema = Type.Object({
    name: Type.String(),
    price: Type.Union([PriceSchema, Type.Null()]),
    priced: Type.Union([
        Type.Literal("unknown"),
        Type.Literal("listed"),
        Type.Literal("assigned"),
        Type.Literal("yours"),
    ]),
    context: Type.Union([Type.Number(), Type.Null()]),
    allowed: Type.Boolean(),
    note: Type.String(),
});
/** The shape of the catalogue changes delivered by pi-durable. */
export const CardRecord = Type.Record(Type.String(), CardSchema);
/** A model's last-known card with the chief's own overrides. */
export type Card = Static<typeof CardSchema>;
/** A provider's model listing before the home's overrides. */
export type Listed = {
    id: string;
    price: Price | null;
    context: number | null;
};
/** The home's last-known model cards and the chief's overrides. */
export const CatalogueDoc = defineDoc<{ models: Record<string, Card> }>({
    kind: "hamlet.catalogue",
    version: 1,
    scope: "session",
    initial: () => ({ models: {} }),
});

/** Merges a new listing without replacing the chief's price, notes or allowance. */
export async function takeIn(
    home: Home,
    provider: string,
    url: string,
    listed: readonly Listed[],
    context: Context,
): Promise<void> {
    await home.harness.commit(async (tx) => {
        const all = (await tx.doc(CatalogueDoc)).models;
        for (const entry of listed) {
            const name = `${provider}::${entry.id}`;
            const old = all[name];
            const card: Card =
                old === undefined
                    ? {
                          name,
                          price: null,
                          priced: "unknown",
                          context: null,
                          allowed: false,
                          note: "",
                      }
                    : {
                          ...old,
                          price: old.price === null ? null : { ...old.price },
                      };
            if (entry.context !== null) card.context = entry.context;
            if (card.priced !== "yours") {
                if (entry.price !== null) {
                    card.price = { ...entry.price };
                    card.priced = "listed";
                } else if (
                    card.priced === "unknown" ||
                    card.priced === "assigned"
                ) {
                    card.price = isLocal(url)
                        ? {
                              input: assumedRate(url),
                              output: assumedRate(url),
                          }
                        : null;
                    card.priced = card.price === null ? "unknown" : "assigned";
                }
            }
            all[name] = card;
        }
    }, context);
}

/** Lists stored model cards in name order. */
export async function catalogueOf(
    home: Home,
    context: Context,
): Promise<readonly Card[]> {
    return Object.values(
        (await home.harness.snapshot(CatalogueDoc, context))?.models ?? {},
    ).sort((a, b) => a.name.localeCompare(b.name));
}

/** Commits the chief's price, allowance and note overrides for a known model. */
export async function editCatalogue(
    home: Home,
    name: string,
    set: { allowed?: boolean; price?: Price | null; note?: string },
    context: Context,
): Promise<Result<void, CatalogueFailure>> {
    if (
        set.price != null &&
        [set.price.input, set.price.output].some(
            (n) => !Number.isFinite(n) || n < 0,
        )
    )
        return err({
            kind: "price-negative",
            operation: "edit-catalogue",
            retrySafe: true,
        });
    const known = (await home.harness.snapshot(CatalogueDoc, context))?.models[
        name
    ];
    const listed = home.models
        .getModels()
        .find((model) => `${model.provider}::${model.id}` === name);
    if (known === undefined && listed === undefined)
        return err({
            kind: "model-unknown",
            name,
            operation: "edit-catalogue",
            retrySafe: true,
        });
    await home.harness.commit(async (tx) => {
        const cards = (await tx.doc(CatalogueDoc)).models;
        let card = cards[name];
        if (card === undefined) {
            if (listed === undefined)
                throw new Error("An admitted model card is missing.");
            cards[name] = {
                name,
                price: { input: listed.cost.input, output: listed.cost.output },
                priced: "listed",
                context: listed.contextWindow,
                allowed: false,
                note: "",
            };
            card = cards[name];
        }
        if (set.allowed !== undefined) card.allowed = set.allowed;
        if (set.note !== undefined) card.note = set.note;
        if (set.price !== undefined) {
            card.price = set.price === null ? null : { ...set.price };
            card.priced = set.price === null ? "unknown" : "yours";
        }
    }, context);
    return ok(undefined);
}

export { type CatalogueFailure, catalogueFailureText } from "./failure.ts";
