import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import { current } from "../chats/conversations.ts";
import { agentEnv } from "../environment.ts";
import type { Home } from "../home.ts";
import type { NotesFailure } from "../house.ts";
import { editLimits, type LimitsRefusal } from "../limits.ts";
import type { CatalogueFailure, SettingsFailure } from "../models.ts";
import { CharacterDoc, editCharter } from "../prompt.ts";
import type { Edit } from "../protocol.ts";
import type { ChiefRefusal } from "../realm/access.ts";
import type { AddressFailure } from "../realm/roads.ts";
import { agentKey } from "../record.ts";
import { err, ok, type Result } from "../result.ts";
import { type WorkspaceConnection, workspaceConnection } from "../settings.ts";
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
    // Scrubbing would replace a token the home already keeps, so the workspace is taken first.
    const workspace = chosenWorkspace(
        message.doc === "settings" ? message.set.workspace : undefined,
    );
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
                // The house path as the agent's own environment names it, so remote edits reach /house.
                const result = await env.writeFile(
                    join(home.houses.of(agent).path, "NOTES.md"),
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
            if (!workspace.ok) return workspace;
            const { workspace: _, ...set } = message.set;
            // An edit of the workspace alone leaves the model settings unsaved.
            if (workspace.value === undefined || Object.keys(set).length > 0) {
                const outcome = await home.modelHome.editSettings(set, context);
                if (!outcome.ok) return outcome;
            }
            if (workspace.value !== undefined)
                await home.credentials.chooseWorkspace(workspace.value);
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

/** The workspace a settings edit chooses: `undefined` leaves it, null is the local one. */
function chosenWorkspace(
    given: { url: string; token: string } | null | undefined,
): Result<WorkspaceConnection | null | undefined, SettingsFailure> {
    if (given === undefined || given === null) return ok(given);
    const connection = workspaceConnection(given.url, given.token);
    if (!connection.ok)
        return err({
            ...connection.error,
            operation: "edit-settings",
            retrySafe: true,
        });
    return connection;
}
