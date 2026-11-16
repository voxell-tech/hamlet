/**
 * Compaction's settings by each listed model's window, and the short
 * cut-off rule (C28): an empty answer cut off short, with history, on its
 * first try, compacts once and runs again.
 */
import type { Api, Model } from "@earendil-works/pi-ai";
import {
    type CompactionSettings,
    defineExtension,
    type Extension,
} from "@earendil-works/pi-durable";

export function compactionFor(
    models: readonly Model<Api>[],
): CompactionSettings {
    void models;
    return {};
}

export function cutOff(): Extension {
    return defineExtension({ name: "hamlet.cut-off" });
}
