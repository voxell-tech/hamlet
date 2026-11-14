// This file was changed for Hamlet. See NOTICE in this package.

import { bufferToArrayBuffer, toUint8Array } from "../utils.ts";
import type { WorkflowHistory } from "./bare/transport/v1.ts";
import {
    decodeWorkflowHistory,
    encodeWorkflowHistory,
} from "./bare/transport/v1.ts";

declare const workflowHistoryBare: unique symbol;

// Branded so these bytes cannot be handed to the CBOR compat encoder, which
// would rewrite the buffer as `["$ArrayBuffer", "<base64>"]`.
export type WorkflowHistoryBytes = ArrayBuffer & {
    readonly [workflowHistoryBare]: true;
};

export function encodeWorkflowHistoryTransport(
    history: WorkflowHistory,
): WorkflowHistoryBytes {
    return bufferToArrayBuffer(
        encodeWorkflowHistory(history),
    ) as WorkflowHistoryBytes;
}

export function decodeWorkflowHistoryTransport(
    data: ArrayBuffer | ArrayBufferView,
): WorkflowHistory {
    return decodeWorkflowHistory(toUint8Array(data));
}
