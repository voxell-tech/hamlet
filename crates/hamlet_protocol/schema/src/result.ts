/** An operation's value or its classified expected failure. */
export type Result<T, E> = { ok: true; value: T } | { ok: false; error: E };

/** A successful outcome carrying the operation's value. */
export function ok<T>(value: T): Result<T, never> {
    return { ok: true, value };
}

/** An expected refusal for the caller to handle. */
export function err<E>(error: E): Result<never, E> {
    return { ok: false, error };
}
