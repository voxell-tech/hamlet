//! A template's town on its page: the view of its turning plinth,
//! or a line while it is laid out or cannot be.

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::viewport;
use hamlet_engine_bevy::ui::{FrameProps, column, keyed, resource};

use crate::diorama::{Diorama, DioramaWorldExt, Seen};
use crate::samples::hamlets::Template;

/// The height of the view, in pixels; it is as wide as the page.
const HEIGHT: f32 = 240.0;

/// The town of `template` turning on its plinth, asked of the realm
/// as the view is built.
pub(crate) fn view(template: &Template) -> AnyView {
    let template = *template;
    AnyView::new(move |cx| {
        cx.world.show_town(&template);
        let name = template.name;
        let seen =
            resource::<Diorama, _>(move |diorama| diorama.seen(name));
        cx.build(keyed(seen, |seen| match seen {
            Seen::Waiting => {
                description("Laying out the town...").boxed()
            }
            Seen::NoRealm => description(
                "The realm is not running, so its town cannot be \
                 shown.",
            )
            .boxed(),
            Seen::Failed(why) => description(format!(
                "The town cannot be shown: {why}"
            ))
            .boxed(),
            Seen::Ready(camera) => {
                column((viewport(*camera),)).height(HEIGHT).boxed()
            }
        }))
    })
}
