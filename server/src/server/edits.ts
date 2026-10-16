import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import { current } from "../chats/conversations.ts";
import { agentEnv } from "../environment.ts";
import { houseOf } from "../home/layout.ts";
import type { Home } from "../home.ts";
import type { NotesFailure } from "../house.ts";
import { editLimits, type LimitsRefusal } from "../limits.ts";
import type { CatalogueFailure, SettingsFailure } from "../models.ts";
import { CharacterDoc, editCharter } from "../prompt.ts";
import type { Edit } from "../protocol.ts";
import type { ChiefRefusal } from "../realm/access.ts";
import type { AddressFailure } from "../realm/roads.ts";
import { agentKey } from "../record.ts";
import { err, type Result } from "../result.ts";
import { readableAgent } from "./reads.ts";

/** A chief's validated document edit, applied before its reply (S6). */
export async function edit(
    home: Home,
    message: Edit,
    context: Context,
): Promise<
    Result<
        string,
        | ChiefRefusal["error"]
        | SettingsFailure
        | CatalogueFailure
        | LimitsRefusal
        | NotesFailure
        | AddressFailure
    >
> {
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
                const env = await agentEnv(
                    home,
                    { conversationId: conversation.id, read: home.harness },
                    { net: false },
                    context,
                );
                if (env === undefined)
                    return err({
                        kind: "notes-unavailable",
                        operation: "write-notes",
                        agent,
                        retrySafe: false,
                    });
                const result = await env.writeFile(
                    join(houseOf(home.layout, agent), "NOTES.md"),
                    text,
                    context,
                );
                if (!result.ok)
                    return err({
                        kind: "notes-write-refused",
                        operation: "write-notes",
                        agent,
                        retrySafe: false,
                        code: result.error.code,
                        text: result.error.message,
                    });
            }
            break;
        }
        case "limits": {
            const outcome = await editLimits(
                home,
                message.hamlet,
                message,
                context,
            );
            if (!outcome.ok) return outcome;
            break;
        }
        case "settings": {
            const outcome = await home.modelHome.editSettings(
                message.set,
                context,
            );
            if (!outcome.ok) return outcome;
            break;
        }
        case "catalogue": {
            const outcome = await home.modelHome.editCatalogue(
                message.model,
                message.set,
                context,
            );
            if (!outcome.ok) return outcome;
            break;
        }
    }
    return { ok: true, value: "Saved." };
}
