/**
 * The library's messages: what an agent asks of the library, and how
 * the agent reads its shelves.
 */
import { arr, obj, type Static, str } from "@earendil-works/pi-schema";
import { Address } from "../../actor/actor/mod.ts";
import type { Piece } from "./profile.ts";
import { type CallId, OPEN, type Refused, reader, U32 } from "./shared.ts";

export type OfferId = number;

export interface Skill {
    /** The skill's folder. */
    readonly name: string;
    readonly description: string;
    readonly usage: string;
}

/** The library's shelves, of one version. */
export interface ShelfList {
    readonly version: number;
    readonly skills: readonly Skill[];
}

/** One part of the files of `skills/<skill>/`, each path from that folder. */
export interface Offer {
    readonly call: CallId;
    readonly skill: string;
    readonly reason: string;
    readonly part: number;
    readonly last: boolean;
    readonly files: readonly Piece[];
}

export type OfferAnswer =
    | { readonly type: "kept"; readonly offer: OfferId }
    | { readonly type: "more" }
    | Refused;

export interface LibraryActions {
    shelves(message: { readonly version_seen?: number }): ShelfList;
    offer(message: Offer): OfferAnswer;
}

export const LIBRARY: Address<LibraryActions> = Address.of("library", "local");

export const SHELF_LIST = obj(
    {
        version: U32,
        skills: arr(
            obj({ name: str(), description: str(), usage: str() }, OPEN),
        ),
    },
    OPEN,
);

export function shelfListOf({
    version,
    skills,
}: Static<typeof SHELF_LIST>): ShelfList {
    return {
        version,
        skills: skills.map(({ name, description, usage }) => ({
            name,
            description,
            usage,
        })),
    };
}

/** The library's shelves, as the agent keeps them. */
export const readShelves = reader(SHELF_LIST, shelfListOf);
