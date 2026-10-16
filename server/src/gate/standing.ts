import type { Tx } from "@earendil-works/pi-durable";
import type { With } from "../chats/conversations.ts";
import { WorkDoc } from "../environment.ts";
import type { Home } from "../home.ts";
import { LimitsDoc } from "../limits/rules.ts";
import { TrustedDoc, trustKey } from "../oversight/asked.ts";
import { addressOf, RecordDoc } from "../record.ts";
import { SettingsDoc, settingsOf } from "../settings.ts";
import { GrantsDoc, grantsOf, holds } from "./grants.ts";
import type { Standing } from "./rule.ts";

/** Current grants, limits, mode and numbered-chat trust, read in the deciding transaction. */
export async function standingOf(
    home: Home,
    tx: Tx,
    place: With,
): Promise<Standing> {
    const work = await tx.doc(WorkDoc, place.hamlet, place.hamlet);
    const record = await tx.doc(RecordDoc, place.hamlet, place.hamlet);
    const by = addressOf(record, place.number);
    const limits = await tx.doc(LimitsDoc, place.hamlet, place.hamlet);
    const settings = settingsOf(await tx.doc(SettingsDoc), home.overrides);
    const grants = grantsOf(await tx.doc(GrantsDoc));
    const key = trustKey(place);
    const trust = await tx.doc(TrustedDoc, key, key);
    const manual = work.folder !== null && work.mode === "manual";
    return {
        confined: true,
        net:
            settings.allowNet ||
            holds(limits.permissions, grants, by, place, "network"),
        unconfined: holds(limits.permissions, grants, by, place, "unconfined"),
        asksEdits: manual && !trust.edit,
        asksCommands: manual && !trust.command,
    };
}
