import type { Tx } from "@earendil-works/pi-durable";
import { ok, type Result } from "../result.ts";
import { type AgentRef, type HamletRecord, RecordDoc } from "./document.ts";
import { type RecordRefusal, refusal } from "./refusals.ts";

/** Confirms a living office holder using the decision's transaction (H27). */
export async function requireMayor(
    tx: Tx,
    place: AgentRef,
): Promise<Result<HamletRecord, RecordRefusal>> {
    const record = await tx.doc(RecordDoc, place.hamlet, place.hamlet);
    return record.mayor === place.number &&
        record.agents.some((a) => a.number === place.number)
        ? ok(record)
        : refusal("mayor", place.hamlet, { kind: "not-mayor" });
}
