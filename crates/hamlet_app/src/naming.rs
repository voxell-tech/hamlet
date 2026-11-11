//! The one way a hamlet, an agent or a scholar is given a new name: a
//! double click on its name at the end of the path of its page in the
//! Hub, which turns it into the field the new name is typed in. Enter
//! keeps it; Escape or a press elsewhere drops it.

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::events::PointerClick;
use bevy::picking::pointer::PointerButton;
use bevy::text::EditableText;
use bevy::ui::px;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::realm::{
    self as agents, Deeds, HamletId, Kind, Left, Scholar,
    scholar_address,
};
use hamlet_protocol::client as protocol;
use hamlet_realm::orders::Decree;
use hamlet_ui::styles::{bare, crumb_here, note, text_input};
use hamlet_ui::{AnyView, built};

use crate::chat;

/// The narrowest the field a new name is typed in is.
const FIELD: f32 = 160.0;

/// What is being given a new name, if anything is.
#[derive(Resource, Default, PartialEq)]
pub struct Naming(pub Option<Entity>);

/// The field a new name is typed in.
#[derive(Component)]
pub(crate) struct NameField;

/// The name of `.0` at the end of its path, which a double click
/// renames.
#[derive(Component)]
struct NameCrumb(Entity);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Naming>()
        .add_observer(twice_clicked)
        .add_systems(
            Update,
            (focus_field, end)
                .chain()
                // Only with a keyboard and its focus, which a
                // headless window has neither of.
                .run_if(resource_exists::<ButtonInput<KeyCode>>)
                .run_if(resource_exists::<InputFocus>),
        );
}

/// What is renamed, and the command that renames it.
enum Named {
    Hamlet(String),
    /// A hire or a worker, by its address.
    Agent(String),
    Scholar(protocol::ScholarId),
}

/// What `named` is, while the chief may rename it: a hamlet, an agent
/// that is not the Hand and has not left, or a scholar that is not
/// archived.
fn named(world: &World, named: Entity) -> Option<Named> {
    if let Some(id) = world.get::<HamletId>(named) {
        return Deeds::is_allowed(world, "name-hamlet")
            .then(|| Named::Hamlet(id.0.clone()));
    }
    if let Some(scholar) = world.get::<Scholar>(named) {
        return (scholar.0.left.is_none()
            && Deeds::is_allowed(world, "name-scholar"))
        .then_some(Named::Scholar(scholar.0.id));
    }
    let kind = world.get::<Kind>(named)?.0;
    (kind != protocol::AgentKind::Hand
        && world.get::<Left>(named).is_none()
        && Deeds::is_allowed(world, "nickname"))
    .then(|| Named::Agent(agents::name_of(world, named)))
}

/// Whether the chief may give `named` a new name.
pub(crate) fn may_rename(world: &World, named: Entity) -> bool {
    self::named(world, named).is_some()
}

/// Gives `named` the name `typed`. A hire or a worker left with no
/// name goes by its own again; a hamlet or a scholar needs one.
fn commit(world: &mut World, named: Entity, typed: &str) {
    let typed = typed.trim();
    match self::named(world, named) {
        Some(Named::Hamlet(id)) if !typed.is_empty() => {
            world.trigger(Decree {
                hamlet: Some(id.clone()),
                text: format!("/name-hamlet {id} {typed}"),
            });
            world.flush();
        }
        Some(Named::Agent(address)) => chat::command(
            world,
            format!("/nickname {address} {typed}")
                .trim_end()
                .to_string(),
        ),
        Some(Named::Scholar(id)) if !typed.is_empty() => {
            chat::command(
                world,
                format!(
                    "/name-scholar {} {typed}",
                    scholar_address(id)
                ),
            );
        }
        _ => {}
    }
}

/// `shown`, the name at the end of the path to `named`, or, while it
/// is renamed, the field its new name is typed in.
pub(crate) fn name_crumb(named: Entity, shown: String) -> AnyView {
    keyed(
        derived(move |world: &World| {
            world
                .get_resource::<Naming>()
                .is_some_and(|n| n.0 == Some(named))
        })
        .when(every_frame()),
        move |open| {
            if !*open {
                let shown = shown.clone();
                return built(move |cx| {
                    let crumb = crumb_here(shown.clone())
                        .tagged(NameCrumb(named));
                    if may_rename(cx.world, named) {
                        cx.build(crumb.tooltip(|| {
                            note("Double-click to rename")
                        }))
                    } else {
                        cx.build(crumb)
                    }
                })
                .boxed();
            }
            let was = shown.clone();
            text_field(shown.clone(), move |world, name| {
                if !in_field(world) {
                    return;
                }
                world.resource_mut::<Naming>().0 = None;
                if name.trim() != was {
                    commit(world, named, &name);
                }
            })
            .min_width(px(FIELD))
            .rules(text_input)
            .tagged(NameField)
            .boxed()
        },
    )
    .within(bare())
    .boxed()
}

/// Renames what has its name clicked twice at the end of its path.
fn twice_clicked(
    click: On<PointerClick>,
    crumbs: Query<&NameCrumb>,
    mut commands: Commands,
) {
    let Ok(crumb) = crumbs.get(click.event_target()) else {
        return;
    };
    if click.button != PointerButton::Primary || click.count != 2 {
        return;
    }
    let named = crumb.0;
    commands.queue(move |world: &mut World| {
        if may_rename(world, named) {
            world.resource_mut::<Naming>().0 = Some(named);
        }
    });
}

/// Puts the keyboard in a new name field.
fn focus_field(
    fields: Query<&Children, Added<NameField>>,
    texts: Query<(), With<EditableText>>,
    mut focus: ResMut<InputFocus>,
) {
    for children in &fields {
        if let Some(&text) =
            children.iter().find(|&&kid| texts.contains(kid))
        {
            focus.set(text, FocusCause::Navigated);
        }
    }
}

/// Whether the keyboard is in a name field, as it is when Enter
/// commits one: a press elsewhere takes it out first.
fn in_field(world: &World) -> bool {
    world
        .get_resource::<InputFocus>()
        .and_then(InputFocus::get)
        .and_then(|input| world.get::<ChildOf>(input))
        .is_some_and(|parent| {
            world.get::<NameField>(parent.parent()).is_some()
        })
}

/// Leaves the name being typed on Escape, or once the keyboard is
/// elsewhere.
fn end(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    fields: Query<Entity, With<NameField>>,
    parents: Query<&ChildOf>,
    mut naming: ResMut<Naming>,
) {
    if naming.0.is_none() {
        return;
    }
    let Ok(field) = fields.single() else {
        return;
    };
    let inside = focus.get().is_some_and(|input| {
        input == field
            || parents
                .iter_ancestors(input)
                .any(|above| above == field)
    });
    if keys.just_pressed(KeyCode::Escape) || !inside {
        naming.0 = None;
    }
}
