import { FileError } from "@earendil-works/pi-durable/env";

/** JSON-line encoding of file results, including bytes and errors. */
export function encode(value: unknown): string {
    return JSON.stringify(value, (_key, part) => {
        if (part instanceof FileError)
            return {
                $error: part.code,
                message: part.message,
                path: part.path,
            };
        if (part instanceof Uint8Array)
            return { $bytes: Buffer.from(part).toString("base64") };
        if (part?.type === "Buffer" && Array.isArray(part.data))
            return { $bytes: Buffer.from(part.data).toString("base64") };
        return part;
    }).replace(/[\u2028\u2029]/g, (char) =>
        char === "\u2028" ? "\\u2028" : "\\u2029",
    );
}

/** The first field of a frame, kept readable even when its body is damaged. */
export function frameId(text: string): number | undefined {
    const found = /^\{"id":(\d+)[,}]/.exec(text);
    if (found === null) return undefined;
    const id = Number(found[1]);
    return Number.isSafeInteger(id) ? id : undefined;
}

/** Decoded file results with their portable error and byte types. */
export function decode(text: string): unknown {
    return JSON.parse(text, (_key, part) => {
        if (part?.$error !== undefined)
            return new FileError(part.$error, part.message, part.path);
        if (part?.$bytes !== undefined)
            return new Uint8Array(Buffer.from(part.$bytes, "base64"));
        return part;
    });
}
