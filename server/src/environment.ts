import type { Context } from "@earendil-works/chord";
import type {
    ConversationId,
    DocumentReader,
} from "@earendil-works/pi-durable";
import { placeOf } from "./environment/place.ts";
import type { HomeBase } from "./home.ts";
import {
    closedPolicy,
    directoryRoot,
    openPolicy,
    policyHash,
    SandboxEnv,
} from "./sandbox.ts";

/** The name of the tool task memo a gate leaves for one call. */
export const LEAVE = "hamlet.leave";

/** The reach a chief's ruling gives one call beyond its standing: the network (C19). */
export type Leave = {
    net: boolean;
};

const environments = new WeakMap<object, Map<string, SandboxEnv>>();

/**
 * The environment of a conversation's tools, under the sandbox (S12, C18). It works in the folder, or in the house
 * when there is none; writes the house and the folder; reads the tools added by hand and the library; and never reads
 * the rest of the home. It reaches the network only with `leave`.
 */
export async function envFor(
    home: Pick<HomeBase, "layout" | "sandbox">,
    target: {
        readonly conversationId: ConversationId;
        readonly read: DocumentReader;
    },
    leave: Leave,
    context: Context,
): Promise<SandboxEnv | undefined> {
    const place = await placeOf(
        home,
        target.read,
        target.conversationId,
        context,
    );
    if (place === undefined) return undefined;
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
    let kept = environments.get(home.layout);
    if (kept === undefined) {
        kept = new Map();
        environments.set(home.layout, kept);
    }
    let env = kept.get(key);
    if (env === undefined) {
        env = new SandboxEnv({ cwd, policy, sandbox: home.sandbox });
        kept.set(key, env);
    }
    return env;
}

/** Closed file helpers and commands belonging to this home. */
export async function closeEnvironments(
    home: Pick<HomeBase, "layout">,
    context: Context,
): Promise<void> {
    const kept = environments.get(home.layout);
    environments.delete(home.layout);
    for (const env of kept?.values() ?? []) await env.cleanup(context);
}

export { footing } from "./environment/footing.ts";
export { type Place, placeOf } from "./environment/place.ts";
export {
    folder,
    modeWords,
    setMode,
    type Work,
    WorkDoc,
} from "./environment/work.ts";
