/** U9 fills this file. Until then the agent's own files have no route. */
import { defineExtension, type Extension } from "@earendil-works/pi-durable";
import type { Peers } from "./turn.ts";

export function ownFiles(_peers: Peers): Extension {
    return defineExtension({ name: "hamlet.own-files" });
}
