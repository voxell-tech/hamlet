// This file was changed for Hamlet. See NOTICE in this package.
// Define minimal event interfaces to avoid conflicts between different WebSocket implementations
export interface SocketEvent {
    type: string;
    target?: any;
    currentTarget?: any;
    /**
     * @experimental
     * Gateway ID for hibernatable websockets (provided by engine envoy)
     **/
    gatewayId?: ArrayBuffer;
    /**
     * @experimental
     * Request ID for hibernatable websockets (provided by engine envoy)
     **/
    requestId?: ArrayBuffer;
}

export interface SocketMessageEvent extends SocketEvent {
    data: any;
    /**
     * @experimental
     * Message index for hibernatable websockets (provided by engine envoy)
     **/
    actorMessageIndex?: number;
}

export interface SocketCloseEvent extends SocketEvent {
    code: number;
    reason: string;
    wasClean: boolean;
}

/**
 * Common WebSocket interface that can be implemented by different WebSocket-like classes
 * This is compatible with the standard WebSocket API but allows for custom implementations
 */
export interface UniversalWebSocket {
    // WebSocket readyState values
    readonly CONNECTING: 0;
    readonly OPEN: 1;
    readonly CLOSING: 2;
    readonly CLOSED: 3;

    // Properties
    readonly readyState: 0 | 1 | 2 | 3;
    binaryType: "arraybuffer" | "blob";
    readonly bufferedAmount: number;
    readonly extensions: string;
    readonly protocol: string;
    readonly url: string;

    // Methods
    send(data: string | ArrayBufferLike | Blob | ArrayBufferView): void;
    close(code?: number, reason?: string): void;
    addEventListener(
        type: string,
        listener: (event: any) => void | Promise<void>,
    ): void;
    removeEventListener(
        type: string,
        listener: (event: any) => void | Promise<void>,
    ): void;
    dispatchEvent(event: SocketEvent): boolean;

    // Event handlers (optional)
    onopen?: ((event: SocketEvent) => void | Promise<void>) | null;
    onclose?: ((event: SocketCloseEvent) => void | Promise<void>) | null;
    onerror?: ((event: SocketEvent) => void | Promise<void>) | null;
    onmessage?: ((event: SocketMessageEvent) => void | Promise<void>) | null;
}
