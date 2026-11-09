//! A small form for a chief's command that takes words: labelled
//! fields over a button that sends what was typed. One form is open
//! at a time, keyed by what it is for.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_ui::styles::{
    self, Looks, bare, lettered, note, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::{AnyView, with_cx};

/// The width of a field's label, so the fields line up.
const LABEL: f32 = 90.0;

/// What the open form is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Asking {
    /// Starting a worker in this hamlet.
    Worker(Entity),
}

/// The form that is open, if one is.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct OpenForm(pub Option<Asking>);

/// The field at a place in the open form.
#[derive(Component)]
struct Field(usize);

/// Opens the form for `asking`, or closes it when it is open.
pub(crate) fn toggle(world: &mut World, asking: Asking) {
    let mut open = world.resource_mut::<OpenForm>();
    open.0 = if open.0 == Some(asking) {
        None
    } else {
        Some(asking)
    };
}

/// What is typed in each field of the open form, in order.
fn values(world: &mut World) -> Vec<String> {
    let mut found = world
        .query::<(&Field, &Children)>()
        .iter(world)
        .map(|(field, children)| {
            (field.0, children.iter().copied().collect::<Vec<_>>())
        })
        .collect::<Vec<_>>();
    found.sort_by_key(|(at, _)| *at);
    found
        .into_iter()
        .map(|(_, children)| {
            children
                .into_iter()
                .find_map(|child| world.get::<EditableText>(child))
                .map(|text| {
                    text.value().to_string().trim().to_string()
                })
                .unwrap_or_default()
        })
        .collect()
}

/// The form for `asking`, shown only while it is the open one: `says`
/// over `fields`, each a label and what to type in it, and `go`
/// naming the button. `send` sends the command of what was typed and
/// returns whether it did; the form closes once it has.
pub(crate) fn form(
    asking: Asking,
    says: &'static str,
    fields: &'static [(&'static str, &'static str)],
    go: &'static str,
    send: impl Fn(&mut World, &[String]) -> bool
    + Clone
    + Send
    + Sync
    + 'static,
) -> AnyView {
    keyed(
        resource::<OpenForm, _>(move |open| open.0 == Some(asking)),
        move |open| {
            if !*open {
                return column(()).boxed();
            }
            let send = send.clone();
            with_cx(move |cx| {
                let gap = cx.theme().space.gap;
                let mut parts = vec![note(says).boxed()];
                for (at, (name, hint)) in fields.iter().enumerate() {
                    parts.push(
                        row((
                            column((label(*name)
                                .tone(Tone::Faint)
                                .wrap(false),))
                            .width(px(LABEL)),
                            text_field(String::new(), |_, _| {})
                                .grow(1.0)
                                .min_width(px(0.0))
                                .rules(text_input)
                                .tagged(Field(at)),
                        ))
                        .gap(gap)
                        .align(AlignItems::Center)
                        .boxed(),
                    );
                    if !hint.is_empty() {
                        parts.push(note(*hint).boxed());
                    }
                }
                let send = send.clone();
                parts.push(
                    row((
                        lettered(go)
                            .variant(Variant::Primary)
                            .on_activate(move |world| {
                                let typed = values(world);
                                if send(world, &typed) {
                                    world
                                        .resource_mut::<OpenForm>()
                                        .0 = None;
                                }
                            }),
                        lettered("Cancel")
                            .rules(styles::button)
                            .on_activate(|world| {
                                world.resource_mut::<OpenForm>().0 =
                                    None;
                            }),
                    ))
                    .gap(gap)
                    .boxed(),
                );
                column(parts).gap(gap)
            })
            .boxed()
        },
    )
    .within(bare())
    .boxed()
}
