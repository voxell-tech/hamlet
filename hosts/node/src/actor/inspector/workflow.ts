// This file was changed for Hamlet. See NOTICE in this package.
import type { JsonCompatValue } from "../common/encoding.ts";
import type { WorkflowHistoryBytes } from "../common/inspector-transport.ts";
import { encodeCborCompat } from "../serde.ts";
import { bufferToArrayBuffer } from "../utils.ts";

export type {
    WorkflowBranchStatus,
    WorkflowCbor,
    WorkflowEntry,
    WorkflowEntryKind,
    WorkflowEntryMetadata,
    WorkflowHistory,
    WorkflowJoinEntry,
    WorkflowLocation,
    WorkflowLoopEntry,
    WorkflowLoopIterationMarker,
    WorkflowMessageEntry,
    WorkflowNameIndex,
    WorkflowPathSegment,
    WorkflowRaceEntry,
    WorkflowRemovedEntry,
    WorkflowRollbackCheckpointEntry,
    WorkflowSleepEntry,
    WorkflowStepEntry,
    WorkflowVersionCheckEntry,
} from "../common/bare/transport/v1.ts";
export {
    WorkflowBranchStatusType,
    WorkflowEntryStatus,
    WorkflowSleepState,
} from "../common/bare/transport/v1.ts";
export type { WorkflowHistoryBytes } from "../common/inspector-transport.ts";
export {
    decodeWorkflowHistoryTransport,
    encodeWorkflowHistoryTransport,
} from "../common/inspector-transport.ts";

/** @experimental State exposed by a durable workflow run handler to the Inspector. */
export type WorkflowState =
    | "pending"
    | "running"
    | "sleeping"
    | "failed"
    | "completed"
    | "cancelled"
    | "rolling_back";

/** @experimental The raw workflow Inspector adapter consumed by the runtime's transport. */
export interface WorkflowInspectorAdapter {
    getHistory: () => WorkflowHistoryBytes | null;
    getState: () => Promise<WorkflowState | null>;
    onHistoryUpdated: (
        listener: (history: WorkflowHistoryBytes) => void,
    ) => () => void;
    replayFromStep: (entryId?: string) => Promise<WorkflowHistoryBytes | null>;
}

/** @experimental Encodes a workflow Inspector value with the runtime's CBOR-compatible codec. */
export function encodeWorkflowInspectorValue(value: unknown): ArrayBuffer {
    return bufferToArrayBuffer(encodeCborCompat(value as JsonCompatValue));
}
