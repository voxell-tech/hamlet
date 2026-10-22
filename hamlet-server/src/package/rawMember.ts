/**
 * Finds the bytes of one top-level member of a JSON object, exactly as
 * sent. The package is stored byte for byte as received, but it arrives
 * inside the upload's JSON body, so it is cut out of the raw body here.
 *
 * Expects text that already parsed as JSON. Works on UTF-8 bytes: every
 * byte of a multi-byte character is >= 0x80, so it never looks like JSON
 * punctuation. With a repeated key the last one wins, as in `JSON.parse`.
 */
export function rawMemberBytes(body: Uint8Array, key: string): Uint8Array | undefined {
  const scanner = new Scanner(body);
  scanner.skipWhitespace();
  if (!scanner.take(OPEN_BRACE)) return undefined;
  let found: Uint8Array | undefined;
  for (;;) {
    scanner.skipWhitespace();
    if (scanner.take(CLOSE_BRACE) || scanner.done()) return found;
    const keyStart = scanner.at;
    scanner.skipString();
    const name = JSON.parse(DECODER.decode(body.subarray(keyStart, scanner.at))) as string;
    scanner.skipWhitespace();
    scanner.take(COLON);
    scanner.skipWhitespace();
    const valueStart = scanner.at;
    scanner.skipValue();
    if (name === key) found = body.subarray(valueStart, scanner.at);
    scanner.skipWhitespace();
    scanner.take(COMMA);
  }
}

const DECODER = new TextDecoder();
const QUOTE = 0x22;
const BACKSLASH = 0x5c;
const OPEN_BRACE = 0x7b;
const CLOSE_BRACE = 0x7d;
const OPEN_BRACKET = 0x5b;
const CLOSE_BRACKET = 0x5d;
const COLON = 0x3a;
const COMMA = 0x2c;
const WHITESPACE = new Set([0x20, 0x09, 0x0a, 0x0d]);

class Scanner {
  at = 0;

  constructor(private readonly bytes: Uint8Array) {}

  done(): boolean {
    return this.at >= this.bytes.length;
  }

  take(byte: number): boolean {
    if (this.bytes[this.at] !== byte) return false;
    this.at += 1;
    return true;
  }

  skipWhitespace(): void {
    while (!this.done() && WHITESPACE.has(this.bytes[this.at]!)) this.at += 1;
  }

  /** At an opening quote; stops just past the closing one. */
  skipString(): void {
    this.at += 1;
    while (!this.done()) {
      const byte = this.bytes[this.at]!;
      this.at += byte === BACKSLASH ? 2 : 1;
      if (byte === QUOTE) return;
    }
  }

  skipValue(): void {
    const first = this.bytes[this.at];
    if (first === QUOTE) return this.skipString();
    if (first === OPEN_BRACE || first === OPEN_BRACKET) return this.skipNested();
    while (!this.done()) {
      const byte = this.bytes[this.at]!;
      if (byte === COMMA || byte === CLOSE_BRACE || byte === CLOSE_BRACKET) return;
      if (WHITESPACE.has(byte)) return;
      this.at += 1;
    }
  }

  private skipNested(): void {
    let depth = 0;
    while (!this.done()) {
      const byte = this.bytes[this.at]!;
      if (byte === QUOTE) {
        this.skipString();
        continue;
      }
      this.at += 1;
      if (byte === OPEN_BRACE || byte === OPEN_BRACKET) depth += 1;
      if (byte === CLOSE_BRACE || byte === CLOSE_BRACKET) depth -= 1;
      if (depth === 0) return;
    }
  }
}
