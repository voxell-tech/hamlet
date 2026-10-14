import { defineDoc } from "@earendil-works/pi-durable";

/** The folder a hamlet works on, and whether its agents ask before changing it (C20, C21). */
export type Work = {
    folder: string | null;
    mode: "manual" | "auto";
};

export const WorkDoc = defineDoc<Work>({
    kind: "hamlet.work",
    version: 1,
    scope: "session",
    initial: () => ({ folder: null, mode: "manual" }),
});
