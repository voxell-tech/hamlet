import { defineEntry } from "@earendil-works/pi-durable";

/** A transcript entry meant only for people, which the model never sees (C30). */
export const NoticeEntry = defineEntry<{ text: string }>("hamlet.notice");
