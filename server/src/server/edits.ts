import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import { current } from "../chats/conversations.ts";
import { envFor } from "../environment.ts";
import { houseOf } from "../home/layout.ts";
import type { Home } from "../home.ts";
import { editLimits } from "../limits.ts";
import { editCatalogue, editSettings } from "../models.ts";
import { CharacterDoc, editCharter } from "../prompt.ts";
import type { Edit } from "../protocol.ts";
import { chiefAccess } from "../realm/access.ts";
import type { AddressFailure } from "../realm/roads.ts";
import { agentKey } from "../record.ts";
import type { Result } from "../result.ts";
import { readableAgent } from "./reads.ts";

/** A chief's validated document edit, applied before its reply (S6). */
export async function edit(
    home: Home,
    message: Edit,
    context: Context,
): Promise<Result<string, AddressFailure>> {
    message = home.secrets.value(message);
    switch (message.doc) {
        case "charter":
            await editCharter(home.harness, message.text, context);
            break;
        case "character":
        case "notes": {
            const resolved = await readableAgent(home, message.agent, context);
            if (!resolved.ok) return resolved;
            const agent = resolved.value;
            const text = message.text;
            if (message.doc === "character") {
                const key = agentKey(agent);
                await home.harness.commit(async (tx) => {
                    (await tx.doc(CharacterDoc, key, key)).text = text;
                }, context);
            } else {
                const conversation = await current(
                    home,
                    { agent, chief: home.chief },
                    context,
                );
                const env = await envFor(
                    home,
                    { conversationId: conversation.id, read: home.harness },
                    { net: false },
                    context,
                );
                if (env === undefined)
                    throw new Error("The agent's house could not be opened.");
                const result = await env.writeFile(
                    join(houseOf(home.layout, agent), "NOTES.md"),
                    text,
                    context,
                );
                if (!result.ok) throw new Error(result.error.message);
            }
            break;
        }
        case "limits": {
            const access = await chiefAccess(
                home,
                message.hamlet,
                "edit limits",
                context,
            );
            if (!access.ok) return access;
            await editLimits(home, message.hamlet, message, context);
            break;
        }
        case "settings":
            await editSettings(home, message.set, context);
            break;
        case "catalogue":
            await editCatalogue(home, message.model, message.set, context);
            break;
    }
    return { ok: true, value: "Saved." };
}
