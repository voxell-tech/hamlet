/** The words a credential is replaced with (C32). */
export const REDACTED = "[REDACTED]";

/**
 * Replaces every known secret in a stream of text with {@link REDACTED}, also one split across chunks. It holds back
 * the end of what it was given while that end could begin a secret, so never more than the longest secret's length
 * less one.
 */
export class Scrubber {
    readonly #secrets: readonly string[];
    readonly #pattern: RegExp | undefined;
    #held = "";

    /** Ignores empty secrets and matches longer overlapping secrets first. */
    constructor(secrets: readonly string[]) {
        // The longest first, where one secret holds another.
        this.#secrets = [...new Set(secrets)]
            .filter((secret) => secret !== "")
            .sort((a, b) => b.length - a.length);
        this.#pattern =
            this.#secrets.length === 0
                ? undefined
                : new RegExp(this.#secrets.map(literally).join("|"), "g");
    }

    /** The text that can go out now of what came so far and `chunk`. */
    push(chunk: string): string {
        if (this.#pattern === undefined) return chunk;
        const text = (this.#held + chunk).replace(this.#pattern, REDACTED);
        const held = this.#beginning(text);
        this.#held = text.slice(text.length - held);
        return text.slice(0, text.length - held);
    }

    /** The text it held back, let out once nothing more comes. */
    flush(): string {
        const held = this.#held;
        this.#held = "";
        return held;
    }

    /** The length of the longest end of `text` that begins a secret. */
    #beginning(text: string): number {
        let longest = 0;
        for (const secret of this.#secrets) {
            for (
                let length = Math.min(secret.length - 1, text.length);
                length > longest;
                length--
            ) {
                if (text.endsWith(secret.slice(0, length))) {
                    longest = length;
                    break;
                }
            }
        }
        return longest;
    }
}

/** A pattern that matches `text` and nothing else. */
function literally(text: string): string {
    return text.replace(/[\\^$.*+?()[\]{}|/-]/g, "\\$&");
}
