import { Buffer } from "node:buffer";

/** The header of a PNG of `width` by `height`, and nothing past it. */
export function png(width: number, height: number): Buffer {
  const header = Buffer.alloc(33);
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]).copy(header, 0);
  header.writeUInt32BE(13, 8);
  header.write("IHDR", 12, "ascii");
  header.writeUInt32BE(width, 16);
  header.writeUInt32BE(height, 20);
  return header;
}

/** The start of a JPEG of `width` by `height`: an APP0 segment, then its frame. */
export function jpeg(width: number, height: number): Buffer {
  const app0 = [0xff, 0xe0, 0x00, 0x10, ...Buffer.from("JFIF\0"), 1, 1, 0, 0, 1, 0, 1, 0, 0];
  const frame = [0xff, 0xc0, 0x00, 0x11, 8, height >> 8, height & 0xff, width >> 8, width & 0xff];
  return Buffer.from([0xff, 0xd8, ...app0, ...frame, 3, ...Array<number>(9).fill(0)]);
}

/** The start of an extended WebP of `width` by `height`. */
export function webp(width: number, height: number): Buffer {
  const header = Buffer.alloc(30);
  header.write("RIFF", 0, "ascii");
  header.writeUInt32LE(22, 4);
  header.write("WEBP", 8, "ascii");
  header.write("VP8X", 12, "ascii");
  header.writeUInt32LE(10, 16);
  header.writeUIntLE(width - 1, 24, 3);
  header.writeUIntLE(height - 1, 27, 3);
  return header;
}
