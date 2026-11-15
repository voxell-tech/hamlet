/** U11 fills this file. Until then no `wake`. */
import { defineExtension, type Extension } from "@earendil-works/pi-durable";

export function timers(): Extension {
    return defineExtension({ name: "hamlet.timers" });
}
