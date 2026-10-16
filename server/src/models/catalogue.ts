import type { Context } from "@earendil-works/chord";
import { defineDoc } from "@earendil-works/pi-durable";
import type { Home } from "../home.ts";
import { assumedRate, isLocal } from "./endpoints.ts";

/** Input and output cost in dollars per million tokens. */
export type Price = { input: number; output: number };
/** A model's known price, capacity and allowance in the home. */
export type Card = {
    name: string;
    price: Price | null;
    priced: "unknown" | "listed" | "assigned" | "yours";
    context: number | null;
    allowed: boolean;
    note: string;
};
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

/** An object value, or an empty object for other representations. */
export function record(value: unknown): Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value)
        ? (value as Record<string, unknown>)
        : {};
}

/** The model cards in an OpenAI-compatible listing, sorted by id. */
export function readList(body: unknown): Listed[] {
    const object = record(body);
    const array = object.data ?? object.models ?? body;
    if (!Array.isArray(array)) return [];
    return array
        .flatMap((item): Listed[] => {
            const value = record(item);
            const id = value.id ?? value.name ?? value.model;
            if (typeof id !== "string" || id === "") return [];
            const pricing = record(value.pricing);
            const input = rate(pricing.prompt);
            const output = rate(pricing.completion);
            const context = value.context_length ?? value.context_window;
            return [
                {
                    id,
                    price:
                        input === null || output === null
                            ? null
                            : { input, output },
                    context:
                        typeof context === "number" && context > 0
                            ? context
                            : null,
                },
            ];
        })
        .sort((a, b) => a.id.localeCompare(b.id));
}
function rate(value: unknown): number | null {
    if (typeof value !== "number" && typeof value !== "string") return null;
    const number = Number(value);
    return value === "" || !Number.isFinite(number) || number < 0
        ? null
        : number * 1_000_000;
}

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
): Promise<void> {
    if (
        set.price != null &&
        [set.price.input, set.price.output].some(
            (n) => !Number.isFinite(n) || n < 0,
        )
    )
        throw new Error("a cost is a number of zero or more");
    await home.harness.commit(async (tx) => {
        const cards = (await tx.doc(CatalogueDoc)).models;
        let card = cards[name];
        if (card === undefined) {
            const listed = home.models
                .getModels()
                .find((model) => `${model.provider}::${model.id}` === name);
            if (listed === undefined)
                throw new Error(`no model named \`${name}\``);
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
}
