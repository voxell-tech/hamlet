import { defineDoc, type JsonObject } from "@earendil-works/pi-durable";

/** A tool a chief added by hand: a program and its manifest in a folder of its own (C17). */
export type Manifest = {
    name: string;
    description: string;
    /** Its folder, which it runs from and can read. */
    dir: string;
    /** The program and its arguments. */
    exec: string[];
    /** The JSON Schema of its arguments. */
    parameters: JsonObject;
    /** True when it asks for the network. */
    net: boolean;
    /** Paths it reads, besides its folder. */
    ro: string[];
    /** Paths it writes. */
    rw: string[];
    env: { [name: string]: string };
    /** Seconds it may run, `null` for the sandbox's default. */
    wallSecs: number | null;
    /** Seconds of CPU it may use, `null` for the sandbox's default. */
    cpuSecs: number | null;
};

/** The tools added by hand that loaded, and a line for each that did not (C15). */
export const ManifestsDoc = defineDoc<{
    tools: Manifest[];
    faults: string[];
}>({
    kind: "hamlet.manifests",
    version: 1,
    scope: "session",
    initial: () => ({ tools: [], faults: [] }),
});
