import type { Context } from "@earendil-works/chord";
import type { DocumentReader } from "@earendil-works/pi-durable";
import { type AgentRef, RecordDoc } from "./record/document.ts";
import { named } from "./record/names.ts";

export {
    type AgentRecord,
    type AgentRef,
    type Archived,
    agentKey,
    type Decision,
    type End,
    FOUNDER,
    type HamletRecord,
    HOME,
    MAYOR,
    RecordDoc,
    type RoadRecord,
} from "./record/document.ts";
export {
    type Admission,
    admit,
    copyArchive,
    depart,
    departing,
    foundRecord,
    lapseTrust,
    mergeable,
    moveMember,
    moving,
    type Restored,
    restore,
    roomFor,
} from "./record/membership.ts";
export {
    addressOf,
    byWords,
    checkName,
    endName,
    farewell,
    named,
    nameOf,
    tidyName,
} from "./record/names.ts";
export {
    type DecisionHeld,
    decisionText,
    mayDecide,
    motionText,
    requireMayor,
    seat,
    type Vote,
    VotesDoc,
} from "./record/office.ts";
export {
    type RecordRefusal,
    recordRefusalText,
    refusal as recordRefusal,
} from "./record/refusals.ts";
export {
    carryRoad,
    hasRoad,
    layRoad,
    level,
    roadsOf,
    takeUpRoad,
} from "./record/roads.ts";
/** The agent an address names, as {@link named} finds it in its hamlet's record. */
export async function addressed(
    read: DocumentReader,
    address: { hamlet: string; agent: string },
    context: Context,
): Promise<AgentRef | undefined> {
    const record = await read.snapshot(RecordDoc, address.hamlet, context);
    if (record === undefined) return undefined;
    const found = named(record, address.agent);
    return found && { hamlet: address.hamlet, number: found.number };
}

export {
    defineMayorTool,
    defineRecordTool,
    memoised,
    type RecordDeed,
    refusedTool,
    toolText,
} from "./record/tools.ts";
