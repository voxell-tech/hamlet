/** U8 fills this file. Until then the gate passes every request through. */
import type { Models } from "@earendil-works/pi-ai";
import type { Peers } from "./turn.ts";

export function gated(models: Models, _peers: Peers): Models {
    return models;
}
