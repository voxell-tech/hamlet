import { createHash } from "node:crypto";
import { mkdir, mkdtemp, rename, rm } from "node:fs/promises";
import { join, posix } from "node:path";
import type { Context } from "@earendil-works/chord";
import { houseOf } from "../home/layout.ts";
import type { Home } from "../home.ts";
import { liveHamlets, RealmDoc } from "../realm/places.ts";
import { type AgentRef, RecordDoc } from "../record.ts";
import { err, ok, type Result } from "../result.ts";
import type { WorkspaceConnection } from "../settings.ts";
import { WorkspaceEnv } from "../workspace.ts";
import { REMOTE_FILE_BYTES, REMOTE_HOUSE_ROOT } from "./remote.ts";
import {
    MERGE_CAPS,
    type StagedEntry,
    stage,
    stagedBytes,
    stagedEntries,
    treeFailureText,
} from "./trees.ts";

/** How far the latest move of the houses has come. */
export type MoveStatus = {
    readonly state: "moving" | "moved" | "failed";
    /** Houses moved so far. */
    readonly done: number;
    /** Houses to move. */
    readonly of: number;
    /** The outcome in words, once there is one. */
    readonly text: string;
    /** Where the houses go: a remote workspace's address, or null for this computer. */
    readonly to: string | null;
};

/** Why the houses were not moved, or stopped moving. */
export type MoveRefusal =
    | { kind: "move-nothing" }
    | { kind: "move-overridden" }
    | { kind: "move-busy" }
    | {
          kind: "move-stopped";
          agent: AgentRef;
          /** Houses moved before this one. */
          moved: number;
          reason: string;
      };

/** The chief's words for a refused or stopped move. */
export function moveRefusalText(refusal: MoveRefusal): string {
    switch (refusal.kind) {
        case "move-nothing":
            return "Hamlet already works in the workspace chosen in Settings, so there are no houses to move.";
        case "move-overridden":
            return "HAMLET_WORKSPACE_URL chooses the workspace for this run, so houses are not moved to the one in Settings.";
        case "move-busy":
            return "The houses are already moving.";
        case "move-stopped":
            return `The house of agent ${refusal.agent.number} in ${refusal.agent.hamlet} was not moved: ${refusal.reason} ${houses(refusal.moved)} before it moved; moving again copies them all once more.`;
    }
}

/**
 * Copies every living agent's house from the workspace the server works in to the one chosen in Settings, which
 * applies from the next start. Each copy replaces the house there and is checked file by file; links and special
 * files are left behind. The houses stay where they are, so what agents change before the restart is not moved.
 * Each step is kept in `home.moving` and `changed` is told.
 */
export async function moveHouses(
    home: Home,
    changed: () => void,
    context: Context,
): Promise<Result<string, MoveRefusal>> {
    if (home.overrides.workspace !== undefined)
        return err({ kind: "move-overridden" });
    const to = home.credentials.workspace;
    if (same(home.workspace, to)) return err({ kind: "move-nothing" });
    if (home.moving.status?.state === "moving")
        return err({ kind: "move-busy" });
    let status: MoveStatus = {
        state: "moving",
        done: 0,
        of: 0,
        text: "",
        to: to?.url ?? null,
    };
    const tell = (next: MoveStatus) => {
        status = next;
        home.moving.status = status;
        changed();
    };
    tell(status);
    try {
        const agents = await livingAgents(home, context);
        tell({ ...status, of: agents.length });
        const left: string[] = [];
        for (const agent of agents) {
            const moved = await moveHouse(home, agent, to, context);
            if (!moved.ok) {
                const refusal: MoveRefusal = {
                    kind: "move-stopped",
                    agent,
                    moved: status.done,
                    reason: moved.error,
                };
                tell({
                    ...status,
                    state: "failed",
                    text: moveRefusalText(refusal),
                });
                return err(refusal);
            }
            left.push(
                ...moved.value.map(
                    (path) => `${agent.hamlet}/${agent.number}/${path}`,
                ),
            );
            tell({ ...status, done: status.done + 1 });
        }
        const text = `Moved ${houses(status.done)} to ${to === null ? "this computer" : "the remote server"}. Restart Hamlet to work there; what agents change before then stays behind.${
            left.length
                ? ` Links and special files are not moved: ${left.slice(0, 5).join(", ")}${left.length > 5 ? `, and ${left.length - 5} more` : ""}.`
                : ""
        }`;
        tell({ ...status, state: "moved", text });
        return ok(text);
    } finally {
        if (status.state === "moving")
            tell({
                ...status,
                state: "failed",
                text: "Moving the houses stopped unexpectedly; moving again copies them all once more.",
            });
    }
}

function houses(count: number): string {
    return count === 1 ? "1 house" : `${count} houses`;
}

/** Whether two choices are the same workspace; a token names one home of a manager. */
function same(
    a: WorkspaceConnection | null,
    b: WorkspaceConnection | null,
): boolean {
    return a === null || b === null
        ? a === b
        : a.url === b.url && a.token === b.token;
}

async function livingAgents(home: Home, context: Context): Promise<AgentRef[]> {
    const realm = await home.harness.snapshot(RealmDoc, context);
    const agents: AgentRef[] = [];
    for (const { id } of liveHamlets(realm)) {
        const record = await home.harness.snapshot(RecordDoc, id, context);
        for (const { number } of record?.agents ?? [])
            agents.push({ hamlet: id, number });
    }
    return agents;
}

/**
 * Stages one house from the running workspace in a server-owned folder, then makes the house in `to` exactly that
 * stage. Returns the links and special files left behind, relative to the house.
 */
async function moveHouse(
    home: Home,
    agent: AgentRef,
    to: WorkspaceConnection | null,
    context: Context,
): Promise<Result<string[], string>> {
    let staged: string;
    try {
        staged = await mkdtemp(join(home.layout.gifts, "move-"));
    } catch {
        return err("its copy could not be staged on this computer.");
    }
    try {
        const copied = await stage(
            home,
            { root: houseOf(home.layout, agent), rel: "." },
            staged,
            MERGE_CAPS,
            context,
            "skip",
        );
        if (!copied.ok) return err(treeFailureText(copied.error));
        const placed =
            to === null
                ? await replaceHere(home, agent, staged)
                : await replaceRemote(
                      new WorkspaceEnv(to, {
                          workspace: agent.hamlet,
                          agent: agent.number,
                          project: false,
                          network: false,
                      }),
                      staged,
                      context,
                  );
        return placed.ok ? ok(copied.value.map((entry) => entry.path)) : placed;
    } finally {
        await rm(staged, { recursive: true, force: true });
    }
}

/** Swaps the stage in for the house on this computer, which agents do not use while the server works remotely. */
async function replaceHere(
    home: Home,
    agent: AgentRef,
    staged: string,
): Promise<Result<void, string>> {
    const house = houseOf(home.layout, agent);
    let slot: string | undefined;
    try {
        await mkdir(join(home.layout.houses, agent.hamlet), {
            recursive: true,
            mode: 0o700,
        });
        slot = await mkdtemp(join(home.layout.erasing, "house-"));
        const old = join(slot, "tree");
        let detached = true;
        await rename(house, old).catch((error: NodeJS.ErrnoException) => {
            if (error.code !== "ENOENT") throw error;
            detached = false;
        });
        try {
            await rename(staged, house);
        } catch (error) {
            if (detached) await rename(old, house);
            throw error;
        }
        return ok(undefined);
    } catch {
        return err("its folder on this computer could not be replaced.");
    } finally {
        if (slot !== undefined)
            await rm(slot, { recursive: true, force: true });
    }
}

/**
 * Makes the remote house exactly the stage: entries that differ or are not staged are removed, missing ones written,
 * and the result is listed again and compared with the stage.
 */
async function replaceRemote(
    env: WorkspaceEnv,
    staged: string,
    context: Context,
): Promise<Result<void, string>> {
    try {
        let wanted: StagedEntry[];
        try {
            wanted = await stagedEntries(staged);
            for (const entry of wanted) {
                if (entry.kind !== "file") continue;
                if (entry.size > REMOTE_FILE_BYTES)
                    return err(
                        `${entry.rel} is larger than ${REMOTE_FILE_BYTES / (1024 * 1024)} MiB, the most a remote workspace takes in one file.`,
                    );
                entry.sha256 = digest(
                    await stagedBytes(join(staged, entry.rel), entry.size),
                );
            }
        } catch {
            return err("its copy on this computer changed while it moved.");
        }
        const made = await env.initialize(context);
        if (!made.ok) return err(made.error.message);
        const listed = await env.tree(REMOTE_HOUSE_ROOT, MERGE_CAPS, context);
        if (!listed.ok) return err(listed.error.message);
        const want = new Map(wanted.map((entry) => [entry.rel, entry]));
        const kept = new Map<string, number>();
        const removed: string[] = [];
        // The listing has each folder before its contents.
        for (const entry of listed.value) {
            if (
                entry.path === "." ||
                removed.some((path) => entry.path.startsWith(`${path}/`))
            )
                continue;
            const staged = want.get(entry.path);
            if (
                staged?.kind === entry.kind &&
                (entry.kind === "directory" ||
                    (staged.sha256 === entry.sha256 &&
                        staged.mode === (entry.mode & 0o777)))
            ) {
                kept.set(entry.path, entry.mode & 0o777);
                continue;
            }
            const gone = await env.remove(
                posix.join(REMOTE_HOUSE_ROOT, entry.path),
                { recursive: true, force: true },
                context,
            );
            if (!gone.ok) return err(gone.error.message);
            removed.push(entry.path);
        }
        for (const entry of wanted) {
            if (entry.rel === "." || kept.has(entry.rel)) continue;
            const path = posix.join(REMOTE_HOUSE_ROOT, entry.rel);
            const written =
                entry.kind === "directory"
                    ? await env.createDir(path, undefined, context)
                    : await env.writeExclusive(
                          path,
                          await stagedBytes(
                              join(staged, entry.rel),
                              entry.size,
                          ),
                          entry.mode,
                          context,
                      );
            if (!written.ok) return err(written.error.message);
        }
        // Folder modes last, so a closed folder still takes its contents.
        const modes = new Map<number, string[]>();
        for (const entry of wanted.toReversed())
            if (
                entry.kind === "directory" &&
                entry.rel !== "." &&
                kept.get(entry.rel) !== entry.mode
            )
                modes.set(entry.mode, [
                    ...(modes.get(entry.mode) ?? []),
                    posix.join(REMOTE_HOUSE_ROOT, entry.rel),
                ]);
        for (const [mode, paths] of modes) {
            const changed = await env.exec(
                ["/usr/bin/chmod", "--", mode.toString(8), ...paths],
                undefined,
                context,
            );
            if (!changed.ok || changed.value.exitCode !== 0)
                return err("its folders' permissions could not be set.");
        }
        const after = await env.tree(REMOTE_HOUSE_ROOT, MERGE_CAPS, context);
        if (!after.ok) return err(after.error.message);
        const found = new Map(after.value.map((entry) => [entry.path, entry]));
        const matches =
            after.value.length === wanted.length &&
            wanted.every((entry) => {
                const actual = found.get(entry.rel);
                return (
                    actual?.kind === entry.kind &&
                    (entry.rel === "." ||
                        (actual.mode & 0o777) === entry.mode) &&
                    (entry.kind === "directory" ||
                        actual.sha256 === entry.sha256)
                );
            });
        return matches
            ? ok(undefined)
            : err(
                  "the copy in the remote workspace did not match it afterwards.",
              );
    } finally {
        await env.cleanup(context);
    }
}

function digest(bytes: Uint8Array): string {
    return createHash("sha256").update(bytes).digest("hex");
}
