import type { Context } from "@earendil-works/chord";
import type {
    ConversationId,
    DocumentReader,
} from "@earendil-works/pi-durable";
import { WithDoc } from "./chats.ts";
import { WorkDoc } from "./hamlet.ts";
import type { HomeBase } from "./home.ts";
import { houseOf } from "./layout.ts";
import { SandboxEnv } from "./sandbox/env.ts";
import { closedPolicy, openPolicy } from "./sandbox/reach.ts";

/** The name of the tool task memo a gate leaves for one call. */
export const LEAVE = "hamlet.leave";

/** The reach a chief's ruling gives one call beyond its standing: the network (C19). */
export type Leave = {
    net: boolean;
};

/** The places an agent's tools work in: its house, and the folder its hamlet works on (C20). */
export type Place = {
    house: string;
    folder: string | null;
};

/** The place of the agent `conversation` is with, `undefined` for a conversation with no agent. */
export async function placeOf(
    home: Pick<HomeBase, "layout">,
    read: DocumentReader,
    conversation: ConversationId,
    context: Context,
): Promise<Place | undefined> {
    const agent = await read.snapshot(WithDoc, conversation, context);
    if (agent === undefined) return undefined;
    const work = await read.snapshot(WorkDoc, agent.hamlet, context);
    return { house: houseOf(home.layout, agent), folder: work?.folder ?? null };
}

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
    return new SandboxEnv({
        cwd: place.folder ?? place.house,
        policy: { ...policy, net: leave.net },
        sandbox: home.sandbox,
    });
}
