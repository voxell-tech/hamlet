/** The flush packet that ends a section of pkt-lines. */
export const FLUSH = Buffer.from("0000");

/** `data` as one pkt-line. */
export function pkt(data: string | Buffer): Buffer {
    const body = typeof data === "string" ? Buffer.from(data) : data;
    const length = (body.length + 4).toString(16).padStart(4, "0");
    return Buffer.concat([Buffer.from(length), body]);
}

/**
 * Reads pkt-lines from the start of `chunks` up to the first flush packet.
 * Returns their payloads and every byte read so far, which may run past the
 * flush; the rest of the stream stays unread in `chunks`.
 */
export async function readSection(
    chunks: AsyncIterator<Buffer>,
): Promise<{ lines: Buffer[]; read: Buffer }> {
    let read = Buffer.alloc(0);
    let offset = 0;
    const want = async (bytes: number) => {
        while (read.length - offset < bytes) {
            const next = await chunks.next();
            if (next.done) throw new Error("The request ended mid-packet.");
            read = Buffer.concat([read, next.value]);
        }
    };
    const lines: Buffer[] = [];
    for (;;) {
        await want(4);
        const length = Number.parseInt(
            read.toString("ascii", offset, offset + 4),
            16,
        );
        if (Number.isNaN(length) || (length > 0 && length < 4)) {
            throw new Error("The request is not made of pkt-lines.");
        }
        if (length === 0) return { lines, read };
        await want(length);
        lines.push(read.subarray(offset + 4, offset + length));
        offset += length;
    }
}
