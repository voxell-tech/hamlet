/** A picture's type and size in pixels, as its header says. */
export interface PictureSize {
  type: "image/png" | "image/jpeg" | "image/webp";
  width: number;
  height: number;
}

/**
 * The type and size of a PNG, JPEG or WebP read from its header alone, or
 * null when the bytes are none of those or their header cannot be read.
 */
export function pictureSize(bytes: Uint8Array): PictureSize | null {
  return png(bytes) ?? jpeg(bytes) ?? webp(bytes);
}

const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

function png(bytes: Uint8Array): PictureSize | null {
  if (bytes.length < 24 || !PNG_SIGNATURE.every((byte, at) => bytes[at] === byte)) return null;
  if (ascii(bytes, 12, 4) !== "IHDR") return null;
  return sized("image/png", u32be(bytes, 16), u32be(bytes, 20));
}

function jpeg(bytes: Uint8Array): PictureSize | null {
  if (bytes[0] !== 0xff || bytes[1] !== 0xd8) return null;
  let at = 2;
  while (at < bytes.length) {
    if (bytes[at] !== 0xff) return null;
    while (bytes[at] === 0xff) at += 1;
    const marker = bytes[at];
    at += 1;
    if (marker === undefined || marker === 0xd9 || marker === 0xda) return null;
    // Markers that stand alone, with no length after them.
    if (marker === 0x01 || marker === 0xd8 || (marker >= 0xd0 && marker <= 0xd7)) continue;
    if (at + 2 > bytes.length) return null;
    const length = u16be(bytes, at);
    if (length < 2) return null;
    const startOfFrame =
      marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc;
    if (startOfFrame) {
      if (at + 7 > bytes.length) return null;
      return sized("image/jpeg", u16be(bytes, at + 5), u16be(bytes, at + 3));
    }
    at += length;
  }
  return null;
}

function webp(bytes: Uint8Array): PictureSize | null {
  if (bytes.length < 30 || ascii(bytes, 0, 4) !== "RIFF" || ascii(bytes, 8, 4) !== "WEBP") {
    return null;
  }
  const at = (offset: number) => bytes[offset] ?? 0;
  switch (ascii(bytes, 12, 4)) {
    case "VP8 ": {
      const keyFrame = (at(20) & 1) === 0;
      const startCode = at(23) === 0x9d && at(24) === 0x01 && at(25) === 0x2a;
      if (!keyFrame || !startCode) return null;
      return sized("image/webp", u16le(bytes, 26) & 0x3fff, u16le(bytes, 28) & 0x3fff);
    }
    case "VP8L": {
      if (at(20) !== 0x2f) return null;
      const bits = at(21) | (at(22) << 8) | (at(23) << 16) | (at(24) << 24);
      return sized("image/webp", (bits & 0x3fff) + 1, ((bits >>> 14) & 0x3fff) + 1);
    }
    case "VP8X":
      return sized("image/webp", u24le(bytes, 24) + 1, u24le(bytes, 27) + 1);
    default:
      return null;
  }
}

function sized(type: PictureSize["type"], width: number, height: number): PictureSize | null {
  return width > 0 && height > 0 ? { type, width, height } : null;
}

function ascii(bytes: Uint8Array, at: number, length: number): string {
  return String.fromCharCode(...bytes.subarray(at, at + length));
}

function u16be(bytes: Uint8Array, at: number): number {
  return ((bytes[at] ?? 0) << 8) | (bytes[at + 1] ?? 0);
}

function u16le(bytes: Uint8Array, at: number): number {
  return (bytes[at] ?? 0) | ((bytes[at + 1] ?? 0) << 8);
}

function u24le(bytes: Uint8Array, at: number): number {
  return (bytes[at] ?? 0) | ((bytes[at + 1] ?? 0) << 8) | ((bytes[at + 2] ?? 0) << 16);
}

function u32be(bytes: Uint8Array, at: number): number {
  return (bytes[at] ?? 0) * 0x1000000 + (u16be(bytes, at + 1) << 8) + (bytes[at + 3] ?? 0);
}
