import { existsSync } from "node:fs";
import { isAbsolute, join, normalize, posix } from "node:path";
import type { Context } from "@earendil-works/chord";
import {
    type ExecutionEnv,
    type FileError,
    ok,
    type Result,
} from "@earendil-works/pi-durable/env";
import type { Place } from "./place.ts";

/**
 * The full path a tool's `path` names (C20). A full path stays as it is. A relative one is the folder's, unless only the
 * house has its first part, as with `NOTES.md`, `skills/` and `inbox/`; with no folder it is the house's.
 */
export function footing(path: string, place: Place): string {
    if (place.remote)
        throw new Error("Virtual tool paths require remoteFooting.");
    if (isAbsolute(path)) return normalize(path);
    if (place.folder === null) return join(place.house, path);
    const first = path.split(/[\\/]/)[0] ?? "";
    const own =
        !existsSync(join(place.folder, first)) &&
        existsSync(join(place.house, first));
    return join(own ? place.house : place.folder, path);
}

/** Resolves C20's house-relative paths using the remote filesystem, never host existence checks. */
export async function remoteFooting(
    path: string,
    place: Place,
    env: ExecutionEnv,
    context: Context,
): Promise<Result<string, FileError>> {
    if (!place.remote) return ok(footing(path, place));
    if (posix.isAbsolute(path) || place.folder === null)
        return ok(posix.resolve(place.house, path));
    const first = path.split("/")[0] ?? "";
    const project = await env.exists(posix.join(place.folder, first), context);
    if (!project.ok) return project;
    if (project.value) return ok(posix.resolve(place.folder, path));
    const own = await env.exists(posix.join(place.house, first), context);
    return own.ok
        ? ok(posix.resolve(own.value ? place.house : place.folder, path))
        : own;
}
