import type { Context } from "@earendil-works/chord";
import type { DocumentReader } from "@earendil-works/pi-durable";
import { localDay } from "../home/clock.ts";
import type { Wait } from "../protocol.ts";
import type { RealmState } from "../realm.ts";
import { addressOf, agentKey } from "../record.ts";
import type { View } from "../state.ts";
import { type How, OversightDayDoc, OversightDoc } from "./waits.ts";
import { queue, says } from "./words.ts";

/** Wait state and the documents whose changes invalidate it. */
export const waitsView: View<
    Wait[],
    Pick<RealmState, "records" | "currentChats">
> = {
    docs: [OversightDoc.definition, OversightDayDoc.definition],
    async build(home, context, { records, currentChats }) {
        const waits = queue(
            await home.harness.snapshot(OversightDoc, context),
        ).map((wait): Wait => {
            const record = wait.chat && records[wait.chat.hamlet];
            return {
                id: wait.id,
                ...(wait.kind === "approval"
                    ? {
                          kind: "approval" as const,
                          danger: wait.danger ?? "unconfined",
                      }
                    : { kind: wait.kind, danger: null }),
                by: wait.by,
                agent: wait.agent && { ...wait.agent },
                says: says(wait.danger),
                about: wait.about,
                change: wait.change && structuredClone(wait.change),
                answers: [...wait.answers],
                since: wait.since,
                asked: wait.asked,
                chat:
                    wait.chat && record
                        ? {
                              with: {
                                  hamlet: wait.chat.hamlet,
                                  agent: addressOf(record, wait.chat.number),
                              },
                              ...(currentChats.get(agentKey(wait.chat))?.[
                                  wait.chat.chief ?? ""
                              ] === wait.chat.chat
                                  ? {}
                                  : { past: wait.chat.chat }),
                          }
                        : null,
            };
        });
        return waits;
    },
};

/** Recorded endings across the local calendar days named by wait identities. */
export async function ended(
    read: DocumentReader,
    ids: readonly string[],
    context: Context,
    at: number,
): Promise<Map<string, How>> {
    const wanted = new Set(ids);
    const times = ids
        .map((id) => Number(id.slice(0, id.lastIndexOf("-"))))
        .filter(Number.isFinite);
    const found = new Map<string, How>();
    if (times.length === 0) return found;
    const day = new Date(Math.min(at, ...times));
    const last = localDay(Math.max(at, ...times));
    while (localDay(day.getTime()) <= last) {
        for (const event of (
            await read.snapshot(
                OversightDayDoc,
                localDay(day.getTime()),
                context,
            )
        )?.events ?? [])
            if (event.type === "settled" && wanted.has(event.id))
                found.set(event.id, event.how);
        day.setDate(day.getDate() + 1);
    }
    return found;
}
