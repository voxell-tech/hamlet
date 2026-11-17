/**
 * What the agent keeps of what the realm owns: the charter, its character,
 * its model and its settings, each with the version the realm gave it.
 * Only `keep` writes the table, and an older or equal version changes
 * nothing.
 */
import type { RawAccess } from "../actor/db/mod.ts";
import type { AgentContext } from "./actor.ts";
import {
    type AgentSettings,
    type Copies,
    type ModelChoice,
    read,
    type ReadCopies as Versions,
} from "./messages.ts";
import type { Peers } from "./turn.ts";

export async function migrateCopies(
    raw: Pick<RawAccess, "execute">,
): Promise<void> {
    await raw.execute(`
        CREATE TABLE IF NOT EXISTS copy (
            name TEXT PRIMARY KEY,
            version INTEGER NOT NULL,
            value TEXT NOT NULL CHECK (json_valid(value))
        ) STRICT
    `);
}

/** The copies the agent holds now. A part it was never sent is absent. */
export interface Held {
    readonly charter?: string;
    readonly character?: string;
    readonly model?: ModelChoice;
    readonly settings?: AgentSettings;
}

export type Part = keyof Copies;

export type ReadCopies = () => Promise<Held>;

/** Keeps each part whose version is newer. */
export async function keep(
    c: Pick<AgentContext, "db">,
    message: Copies,
): Promise<void> {
    for (const part of Object.keys(message) as Part[]) {
        const copy = message[part];
        if (copy === undefined) continue;
        await c.db.run(
            `INSERT INTO copy (name, version, value) VALUES (?, ?, ?)
             ON CONFLICT (name) DO UPDATE SET version = excluded.version, value = excluded.value
             WHERE excluded.version > copy.version`,
            part,
            copy.version,
            JSON.stringify(copy.value),
        );
    }
}

export function copiesOf(db: AgentContext["db"]): ReadCopies {
    return async () => {
        const rows = await db.all<{ name: string; value: string }>(
            "SELECT name, value FROM copy",
        );
        return Object.fromEntries(
            rows.map((row) => [row.name, JSON.parse(row.value)]),
        ) as Held;
    };
}

async function versionsOf(db: AgentContext["db"]): Promise<Versions> {
    const rows = await db.all<{ name: string; version: number }>(
        "SELECT name, version FROM copy",
    );
    return Object.fromEntries(rows.map((row) => [row.name, row.version]));
}

/** Asks the realm for the parts newer than those held and keeps them. */
export async function refreshed(
    c: Pick<AgentContext, "db">,
    peers: Peers,
): Promise<void> {
    const newer = await peers.reach.read_copies(await versionsOf(c.db));
    await keep(c, read.copies(newer));
}
