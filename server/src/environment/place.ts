import type { Context } from "@earendil-works/chord";
import type {
    ConversationId,
    DocumentReader,
} from "@earendil-works/pi-durable";
import { WithDoc } from "../chats/conversations.ts";
import { houseOf } from "../home/layout.ts";
import type { HomeBase } from "../home.ts";
import { WorkDoc } from "./work.ts";

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
