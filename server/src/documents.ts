import type { Context } from "@earendil-works/chord";
import type {
    JsonObject,
    Session,
    SessionDocToken,
} from "@earendil-works/pi-durable";

/** Sets the given fields of a home-wide document in one commit. */
export async function editDoc<T extends JsonObject>(
    session: Session,
    token: SessionDocToken<T>,
    change: Partial<T>,
    context: Context,
): Promise<void> {
    await session.commit(async (tx) => {
        Object.assign(await tx.doc(token), change);
    }, context);
}
