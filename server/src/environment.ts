import { randomUUID } from "node:crypto";
import { fileURLToPath } from "node:url";
import type { Context } from "@earendil-works/chord";
import type {
    ConversationId,
    DocumentReader,
} from "@earendil-works/pi-durable";
import { WithDoc } from "./chats/conversations.ts";
import { placeOf } from "./environment/place.ts";
import type { Manifest } from "./hand/manifests.ts";
import { houseOf } from "./home/layout.ts";
import type { HomeBase } from "./home.ts";
import { librarySnapshot } from "./library/shelf.ts";
import type { AgentRef } from "./record.ts";
import { canon, directoryRoot, policyHash, within } from "./sandbox/policy.ts";
import { closedPolicy, openPolicy } from "./sandbox/reach.ts";
import { SandboxEnv } from "./sandbox.ts";
import { WorkspaceEnv } from "./workspace.ts";

/** The name of the tool task memo a gate leaves for one call. */
export const LEAVE = "hamlet.leave";

/** The reach a chief's ruling gives one call beyond its standing: the network (C19). */
export type Leave = {
    net: boolean;
};

/** Conversation environments whose helpers close with their home. */
export class Confinement {
    readonly #environments = new Map<string, SandboxEnv | WorkspaceEnv>();

    /** Reuses only environments with the same working directory and complete policy. */
    keep(
        key: string,
        create: () => SandboxEnv | WorkspaceEnv,
    ): SandboxEnv | WorkspaceEnv {
        let env = this.#environments.get(key);
        if (env === undefined) {
            env = create();
            this.#environments.set(key, env);
        }
        return env;
    }

    /** Closes every cached file helper. */
    async close(context: Context): Promise<void> {
        for (const env of this.#environments.values())
            await env.cleanup(context);
        this.#environments.clear();
    }
}

/**
 * The environment of a conversation's tools, under the sandbox (S12, C18). It works in the folder, or in the house
 * when there is none; writes the house and the folder; reads the tools added by hand and the library; and never reads
 * the rest of the home. It reaches the network only with `leave`.
 */
export async function agentEnv(
    home: Pick<HomeBase, "layout" | "sandbox" | "confinement" | "overrides">,
    target: {
        readonly conversationId: ConversationId;
        readonly read: DocumentReader;
    },
    leave: Leave,
    context: Context,
): Promise<SandboxEnv | WorkspaceEnv | undefined> {
    const place = await placeOf(
        home,
        target.read,
        target.conversationId,
        context,
    );
    if (place === undefined) return undefined;
    const connection = home.overrides.workspace;
    if (connection !== undefined) {
        const agent = await target.read.snapshot(
            WithDoc,
            target.conversationId,
            context,
        );
        if (agent === undefined) return undefined;
        const identity = {
            workspace: agent.hamlet,
            agent: agent.number,
            project: place.folder !== null,
            network: leave.net,
        };
        return home.confinement.keep(
            `remote:${JSON.stringify(identity)}`,
            () =>
                new WorkspaceEnv(connection, identity, async (context) => {
                    const reader = treeWorker(home, {
                        root: home.layout.library,
                        caps: { entries: 1024, bytes: 4 * 1024 * 1024 },
                    });
                    try {
                        return await librarySnapshot(
                            home.layout.library,
                            reader,
                            context,
                        );
                    } finally {
                        await reader.cleanup(context);
                    }
                }),
        );
    }
    directoryRoot(place.house);
    if (place.folder !== null) directoryRoot(place.folder);
    const shared = [home.layout.tools, home.layout.library];
    const policy =
        place.folder === null
            ? closedPolicy(place.house, shared)
            : openPolicy({
                  folder: place.folder,
                  house: place.house,
                  home: home.layout.dir,
                  shared,
              });
    policy.net = leave.net;
    const cwd = place.folder ?? place.house;
    const key = `${cwd}:${policyHash(policy, home.sandbox.name)}`;
    return home.confinement.keep(
        key,
        () => new SandboxEnv({ cwd, policy, sandbox: home.sandbox }),
    );
}

/** A read-only house environment; the caller closes its helper. */
export function houseReader(
    home: Pick<HomeBase, "layout" | "sandbox">,
    agent: AgentRef | { path: string },
    _context: Context,
): SandboxEnv {
    const root = "path" in agent ? agent.path : houseOf(home.layout, agent);
    directoryRoot(root);
    const policy = closedPolicy(root, [root, process.execPath]);
    policy.rw = [];
    return new SandboxEnv({
        cwd: root,
        policy,
        sandbox: home.sandbox,
        tmpScope: `papers-${randomUUID()}`,
    });
}

/** A closed tree worker; only its stage is writable and its source is readable. */
export function treeWorker(
    home: Pick<HomeBase, "sandbox">,
    options: {
        root: string;
        stage?: string;
        caps: { bytes: number; entries: number };
    },
): SandboxEnv {
    const { root, stage, caps } = options;
    directoryRoot(root);
    if (stage !== undefined) directoryRoot(stage);
    const cwd = stage ?? root;
    const policy = closedPolicy(cwd, [
        root,
        process.execPath,
        fileURLToPath(new URL("./sandbox/tree-helper.ts", import.meta.url)),
        fileURLToPath(new URL("../package.json", import.meta.url)),
    ]);
    policy.rw = stage === undefined ? [] : [stage];
    policy.env = { PATH: "/usr/bin:/bin", HOME: cwd, LANG: "C.UTF-8" };
    policy.limits = {
        maxFileMb: Math.max(1, Math.ceil(caps.bytes / (1024 * 1024)) + 1),
        wallSecs: 120,
    };
    return new SandboxEnv({
        cwd,
        policy,
        sandbox: home.sandbox,
        tmpScope: `trees-${randomUUID()}`,
    });
}

/** A hand tool's declared reach with server-owned folders protected from writes. */
export function toolRunner(
    home: Pick<HomeBase, "layout" | "sandbox">,
    manifest: Manifest,
    net: boolean,
    scope: string,
): SandboxEnv {
    return new SandboxEnv({
        cwd: manifest.dir,
        sandbox: home.sandbox,
        tmpScope: scope,
        policy: {
            ro: manifest.ro,
            rw: manifest.rw,
            open: false,
            hidden: [],
            locked: [
                home.layout.tools,
                manifest.dir,
                home.layout.gifts,
                home.layout.erasing,
            ],
            net: manifest.net && net,
            env: { ...home.sandbox.baseEnv(), ...manifest.env },
            limits: {
                ...(manifest.wallSecs !== null && {
                    wallSecs: manifest.wallSecs,
                }),
                ...(manifest.cpuSecs !== null && { cpuSecs: manifest.cpuSecs }),
            },
        },
    });
}

/** Canonical house containment for the manual-mode edit exemption. */
export function insideHouse(path: string, house: string): boolean {
    return within(canon(path), canon(house));
}

export { footing, remoteFooting } from "./environment/footing.ts";
export { type Place, placeOf } from "./environment/place.ts";
export {
    folder,
    modeWords,
    setMode,
    type Work,
    WorkDoc,
    type WorkFailure,
    workFailureText,
} from "./environment/work.ts";
