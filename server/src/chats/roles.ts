import {
    type ConversationId,
    configure,
    type Tx,
} from "@earendil-works/pi-durable";
import type { Home } from "../home.ts";
import { liveHamlets, RealmDoc } from "../realm/places.ts";
import { RecordDoc } from "../record.ts";
import { WithDoc } from "./conversations.ts";
import { ErrandDoc } from "./errands.ts";

const MAYOR = [
    "create_agent",
    "build_road",
    "set_model",
    "remove_agent",
    "merge_agents",
];
const REALM = ["agree_road", "agree_move", "ask_removal"];
const EVERY = [
    "tell_agent",
    "ask_agent",
    "call_vote",
    "submit_skill",
    "ask_librarian",
];
/** Society tools governed by the role table (H35). */
export const SOCIETY_TOOLS = [
    ...MAYOR,
    ...REALM,
    ...EVERY,
    "request_road",
    "object",
    "ask_chief",
    "cast_ballot",
];
/** The role's tool filter, applied to enabled tools as well as model offers. */
export async function configureRole(
    home: Home,
    tx: Tx,
    conversation: ConversationId,
): Promise<void> {
    const place = await tx.doc(WithDoc, conversation);
    const errand = await tx.doc(ErrandDoc, conversation);
    const record = await tx.doc(RecordDoc, place.hamlet, place.hamlet);
    const allowed = new Set(
        errand.kind !== null
            ? errand.kind === "ballot"
                ? ["cast_ballot"]
                : []
            : [
                  ...EVERY,
                  "ask_chief",
                  ...(place.chief === null ? [] : ["object"]),
                  ...(record.mayor === place.number
                      ? [
                            ...MAYOR,
                            ...(liveHamlets(await tx.doc(RealmDoc)).length > 1
                                ? REALM
                                : []),
                        ]
                      : ["request_road"]),
              ],
    );
    const tools = home.registry
        .snapshot()
        .tools()
        .map(({ tool }) => tool);
    await configure(
        tx,
        conversation,
        errand.kind !== null
            ? { tools: tools.filter((t) => allowed.has(t.name)) }
            : {
                  tools: {
                      remove: tools.filter(
                          (t) =>
                              SOCIETY_TOOLS.includes(t.name) &&
                              !allowed.has(t.name),
                      ),
                  },
              },
    );
}
