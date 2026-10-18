import { isAbsolute, posix, relative, resolve, sep } from "node:path";
import type { HomeBase } from "../home.ts";
import type { AgentRef } from "../record.ts";
import { WorkspaceEnv } from "../workspace.ts";

/** Where every remote house appears inside its own workspace environment. */
export const REMOTE_HOUSE_ROOT = "/house";

/** The most bytes one remote file transfer carries; larger files are refused, never truncated. */
export const REMOTE_FILE_BYTES = 4 * 1024 * 1024;

/** One agent's house in the workspace service, decoded from its server-side label. */
export type RemoteHouse = {
    /** The agent whose workspace holds the house. */
    readonly agent: AgentRef;
    /** The decoded path in the workspace: `/house` or a path below it. */
    readonly path: string;
    /** A fresh network-denied environment over this house; the caller cleans it up. */
    open(): WorkspaceEnv;
};

/**
 * Where a server path lives. `host` is a local home, or a server-owned folder outside every house in remote mode;
 * `remote` is a house in the workspace service; `unmappable` is a remote-mode path that must not be touched at all.
 */
export type HousePlace =
    | { kind: "host" }
    | { kind: "remote"; house: RemoteHouse }
    | { kind: "unmappable" };

const WORKSPACE = /^[A-Za-z0-9_-]{1,128}$/;
const NUMBER = /^(0|[1-9][0-9]{0,9})$/;

/**
 * Decodes `layout.houses/<hamlet>/<number>[/...]` into its workspace identity and `/house` path. The decoding is
 * lexical: in remote mode no house label is ever inspected on the host. Paths that are relative, not normalized,
 * above the houses folder or directly inside it, or that name an invalid hamlet or number, are `unmappable`.
 */
export function remoteHouse(
    home: Pick<HomeBase, "layout" | "overrides">,
    path: string,
): HousePlace {
    const connection = home.overrides.workspace;
    if (connection === undefined) return { kind: "host" };
    if (!isAbsolute(path) || path.includes("\0")) return { kind: "unmappable" };
    const normal = resolve(path);
    // Only an already normal path is decoded; a trailing separator is the one other spelling allowed.
    if (path !== normal && path !== normal + sep) return { kind: "unmappable" };
    const houses = resolve(home.layout.houses);
    // The houses folder itself, or a folder above it, would contain every house.
    if (
        houses === normal ||
        houses.startsWith(normal === sep ? sep : normal + sep)
    )
        return { kind: "unmappable" };
    const rel = relative(houses, normal);
    if (rel === ".." || rel.startsWith(`..${sep}`) || isAbsolute(rel))
        return { kind: "host" };
    const [hamlet = "", number = "", ...rest] = rel.split(sep);
    if (!WORKSPACE.test(hamlet) || !NUMBER.test(number))
        return { kind: "unmappable" };
    const agent = Number(number);
    if (agent > 0xffff_ffff) return { kind: "unmappable" };
    const ref: AgentRef = { hamlet, number: agent };
    return {
        kind: "remote",
        house: {
            agent: ref,
            path: posix.join(REMOTE_HOUSE_ROOT, ...rest),
            open: () =>
                new WorkspaceEnv(connection, {
                    workspace: hamlet,
                    agent,
                    project: false,
                    network: false,
                }),
        },
    };
}
