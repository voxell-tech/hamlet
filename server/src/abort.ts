import type { Context } from "@earendil-works/chord";

/** Rejects with the abort's reason once `context` is aborted. */
export function untilAborted(context: Context): Promise<never> {
    return new Promise((_resolve, reject) => {
        const signal = context.abortSignal;
        if (signal === undefined) {
            reject(new Error("The work cannot be stopped without its signal."));
        } else if (signal.aborted) {
            reject(signal.reason);
        } else {
            signal.addEventListener("abort", () => reject(signal.reason), {
                once: true,
            });
        }
    });
}
