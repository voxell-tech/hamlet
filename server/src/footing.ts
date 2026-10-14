import { existsSync } from "node:fs";
import { isAbsolute, join, normalize } from "node:path";
import type { Place } from "./environment.ts";

/**
 * The full path a tool's `path` names (C20). A full path stays as it is. A relative one is the folder's, unless only the
 * house has its first part, as with `NOTES.md`, `skills/` and `inbox/`; with no folder it is the house's.
 */
export function footing(path: string, place: Place): string {
    if (isAbsolute(path)) return normalize(path);
    if (place.folder === null) return join(place.house, path);
    const first = path.split(/[\\/]/)[0] ?? "";
    const own =
        !existsSync(join(place.folder, first)) &&
        existsSync(join(place.house, first));
    return join(own ? place.house : place.folder, path);
}
