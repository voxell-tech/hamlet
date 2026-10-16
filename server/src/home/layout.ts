import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import type { AgentRef } from "../hamlet.ts";
import { directoryRoot } from "../sandbox/policy.ts";

/** The paths of a home's parts (C8). */
export type Layout = {
    readonly dir: string;
    /** The home's storage. */
    readonly storage: string;
    /** Every credential, readable only by its owner (S7). */
    readonly credentials: string;
    /** pi-ai's stored model lists. */
    readonly models: string;
    /** The file in which a running server gives its address. */
    readonly server: string;
    /** The skills every agent shares (H33). */
    readonly library: string;
    /** The tools a chief added by hand (C17). */
    readonly tools: string;
    readonly houses: string;
    readonly librarian: string;
    readonly gifts: string;
    readonly erasing: string;
};

/** The layout of the home in `dir`. */
export function layoutOf(dir: string): Layout {
    return {
        dir,
        storage: join(dir, "home.sqlite"),
        credentials: join(dir, "credentials.json"),
        models: join(dir, "models.json"),
        server: join(dir, "server.json"),
        library: join(dir, "library"),
        tools: join(dir, "tools"),
        houses: join(dir, "houses"),
        librarian: join(dir, "librarian"),
        gifts: join(dir, ".gifts"),
        erasing: join(dir, ".erasing"),
    };
}

/** Lays out the home in `dir`, keeping whatever is already there. */
export async function layOut(dir: string): Promise<Layout> {
    const layout = layoutOf(dir);
    for (const folder of [
        layout.library,
        layout.tools,
        layout.houses,
        layout.librarian,
        layout.gifts,
        layout.erasing,
    ]) {
        directoryRoot(folder);
        await mkdir(folder, { recursive: true, mode: 0o700 });
        directoryRoot(folder);
    }
    try {
        await writeFile(join(layout.librarian, "NOTES.md"), "# Notes\n", {
            flag: "wx",
        });
    } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
    }
    return layout;
}

/** An agent's house: its whole writable workspace. */
export function houseOf(layout: Layout, agent: AgentRef): string {
    return join(layout.houses, agent.hamlet, String(agent.number));
}

/** Builds `agent`'s house, keeping whatever is already there, and returns its path. */
export async function buildHouse(
    layout: Layout,
    agent: AgentRef,
): Promise<string> {
    const house = houseOf(layout, agent);
    for (const folder of [
        layout.houses,
        join(layout.houses, agent.hamlet),
        house,
        join(house, "skills"),
        join(house, "inbox"),
    ]) {
        directoryRoot(folder);
        await mkdir(folder, { recursive: true, mode: 0o700 });
        directoryRoot(folder);
    }
    try {
        await writeFile(join(house, "NOTES.md"), "# Notes\n", { flag: "wx" });
    } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
    }
    return house;
}
