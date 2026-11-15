/** U10 fills this file. Until then no society tool and no reply. */
import { defineExtension, type Extension } from "@earendil-works/pi-durable";
import type { SocietyTool } from "./messages.ts";
import type { Peers } from "./turn.ts";

export function society(
    _peers: Peers,
    _off: readonly SocietyTool[],
): Extension {
    return defineExtension({ name: "hamlet.society" });
}
