//! The typefaces, the keyboard focus and the small rules of a node
//! that fynix leaves to the app.

use bevy::app::{App, Startup, Update};
use bevy::asset::{AssetId, Assets, Handle};
use bevy::color::Color;
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, Changed, Has, Or, With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{
    FocusCause, FocusLost, InputFocus, InputFocusVisible,
};
use bevy::picking::events::{Pointer, Press};
use bevy::text::{
    DEFAULT_FONT_DATA, EditableText, Font, FontCx, FontSize,
    FontSource, LetterSpacing, TextFont,
};
use bevy::ui::{
    BackgroundGradient, ColorStop, FlexWrap, LinearGradient, Node,
    Outline, OverflowClipMargin, percent, px,
};
use bevy::ui_widgets::{
    Button as ButtonBehavior, Checkbox as CheckboxBehavior,
    MenuButton,
};
use bevy_fynix::dock::DockTab;
use bevy_fynix::tokens::{TextTokens, Tone};
use hamlet_assets::fonts::{CINZEL, INTER, TWEMOJI};

use crate::theme::Themed;

/// The spacing of the letters of a header.
const TRACKING: LetterSpacing = LetterSpacing::Px(1.0);

/// The size of a tab's name.
const TAB_SIZE: FontSize = FontSize::Px(12.0);

type Unringed = (
    Or<(
        With<ButtonBehavior>,
        With<CheckboxBehavior>,
        With<MenuButton>,
    )>,
    Without<Ring>,
);

pub fn plugin(app: &mut App) {
    app.add_observer(hide_ring_on_press)
        .add_observer(take_typed_on_press)
        .add_systems(Startup, install_fonts)
        .add_systems(
            Update,
            (
                apply_mono,
                apply_display,
                apply_glow,
                apply_clip,
                apply_wrap,
                (make_focusable, focus_ring.run_if(focus_moved))
                    .chain(),
            ),
        );
}

/// The display face, for headers and buttons.
#[derive(Resource)]
pub struct Display(pub(crate) Handle<Font>);

/// A label drawn in the display face.
#[derive(Component)]
pub struct DisplayText;

/// The monospace face, kept for what the machine ran.
#[derive(Resource)]
pub struct Mono(pub(crate) Handle<Font>);

/// A label drawn in the monospace face.
#[derive(Component)]
pub struct MonoText;

/// Makes Inter the default font, so every label and field uses it,
/// and keeps the bundled monospace face for [`MonoText`].
pub fn install_fonts(world: &mut World) {
    let mut fonts = world.resource_mut::<Assets<Font>>();
    let _ = fonts.insert(
        AssetId::default(),
        Font::from_bytes(INTER.bytes.to_vec()),
    );
    let mono =
        fonts.add(Font::from_bytes(DEFAULT_FONT_DATA.to_vec()));
    let display = fonts.add(Font::from_bytes(CINZEL.bytes.to_vec()));
    world.insert_resource(Mono(mono));
    world.insert_resource(Display(display));
    install_emoji(&mut world.resource_mut::<FontCx>());
}

/// Makes Twemoji the emoji family.
fn install_emoji(cx: &mut FontCx) {
    // Registered here, since a family is named only once it is in
    // the collection, and a font asset is not until the first layout.
    let emoji = Font::from_bytes(TWEMOJI.bytes.to_vec());
    let added = cx.collection.register_fonts(emoji.data, None);
    let name = added
        .first()
        .and_then(|(family, _)| cx.collection.family_name(*family))
        .map(str::to_owned);
    if let Some(name) = name {
        let _ = cx.set_emoji_family(&name);
    }
}

/// Sets the display face, tracked, on tagged labels and on the dock's
/// tab names, when a label is new or was styled again.
pub fn apply_display(
    mut commands: Commands,
    display: Res<Display>,
    tabs: Query<(), With<DockTab>>,
    parents: Query<&ChildOf>,
    mut fonts: Query<
        (
            Entity,
            &mut TextFont,
            Option<&LetterSpacing>,
            Has<DisplayText>,
        ),
        Or<(Changed<TextFont>, Added<DisplayText>)>,
    >,
) {
    for (node, mut font, spacing, tagged) in &mut fonts {
        let tab = parents
            .iter_ancestors(node)
            .any(|above| tabs.contains(above));
        if !tagged && !tab {
            continue;
        }
        if !matches!(&font.font, FontSource::Handle(h) if *h == display.0)
        {
            font.font = FontSource::Handle(display.0.clone());
        }
        if tab && font.font_size != TAB_SIZE {
            font.font_size = TAB_SIZE;
        }
        if spacing != Some(&TRACKING) {
            commands.entity(node).try_insert(TRACKING);
        }
    }
}

/// A clipping node that cuts its contents at its padding.
#[derive(Component)]
pub struct ClipInside;

/// Cuts each new [`ClipInside`] node at its padding.
pub fn apply_clip(mut nodes: Query<&mut Node, Added<ClipInside>>) {
    for mut node in &mut nodes {
        node.overflow_clip_margin = OverflowClipMargin::content_box();
    }
}

/// A row whose things go on to a line below when it is too narrow for
/// them.
#[derive(Component)]
pub struct Wraps;

/// Lets each new [`Wraps`] row wrap.
pub(crate) fn apply_wrap(mut rows: Query<&mut Node, Added<Wraps>>) {
    for mut row in &mut rows {
        row.flex_wrap = FlexWrap::Wrap;
        // The lines are as far apart as the things in one.
        row.row_gap = row.column_gap;
    }
}

/// A surface whose fill warms toward its lower edge.
#[derive(Component)]
pub struct Glow;

/// Gives each [`Glow`] surface its gradient.
pub fn apply_glow(
    mut commands: Commands,
    theme: Themed,
    surfaces: Query<
        Entity,
        (With<Glow>, Without<BackgroundGradient>),
    >,
) {
    for surface in &surfaces {
        let stops = vec![
            ColorStop::new(theme.palette.black, percent(35.0)),
            ColorStop::new(theme.palette.hearth, percent(100.0)),
        ];
        commands.entity(surface).try_insert(
            BackgroundGradient::from(LinearGradient::to_bottom(
                stops,
            )),
        );
    }
}

/// Sets the monospace face on tagged labels.
pub fn apply_mono(
    mono: Res<Mono>,
    mut labels: Query<
        &mut TextFont,
        (With<MonoText>, Or<(Changed<TextFont>, Added<MonoText>)>),
    >,
) {
    for mut font in &mut labels {
        if !matches!(&font.font, FontSource::Handle(h) if *h == mono.0)
        {
            font.font = FontSource::Handle(mono.0.clone());
        }
    }
}

/// A control that takes keyboard focus and shows a ring while it has
/// it.
#[derive(Component)]
pub struct Ring;

/// Puts every button, checkbox and dropdown button in the Tab order,
/// with an outline to show focus.
pub fn make_focusable(
    mut commands: Commands,
    controls: Query<(Entity, Has<TabIndex>), Unringed>,
) {
    for (control, tabbable) in &controls {
        let mut entity = commands.entity(control);
        entity.insert((
            Ring,
            Outline::new(px(0.0), px(1.0), Color::NONE),
        ));
        if !tabbable {
            entity.insert(TabIndex(0));
        }
    }
}

/// The focus moved, was shown or hidden, or a control was given its
/// ring.
pub(crate) fn focus_moved(
    focus: Res<InputFocus>,
    visible: Res<InputFocusVisible>,
    ringed: Query<(), Added<Ring>>,
) -> bool {
    focus.is_changed() || visible.is_changed() || !ringed.is_empty()
}

/// Draws the ring only after a keyboard move, never after a click.
pub fn focus_ring(
    focus: Res<InputFocus>,
    visible: Res<InputFocusVisible>,
    theme: Themed,
    mut rings: Query<(Entity, &mut Outline), With<Ring>>,
) {
    let accent = theme.tone(Tone::Accent);
    for (control, mut outline) in &mut rings {
        let shown = visible.0 && focus.get() == Some(control);
        let want = if shown {
            Outline::new(px(2.0), px(1.0), accent)
        } else {
            Outline::new(px(0.0), px(1.0), Color::NONE)
        };
        if *outline != want {
            *outline = want;
        }
    }
}

pub(crate) fn hide_ring_on_press(
    _: On<Pointer<Press>>,
    mut visible: ResMut<InputFocusVisible>,
) {
    if visible.0 {
        visible.0 = false;
    }
}

/// Hands over the text of the field being typed in when anything
/// outside it is pressed, before the press does its work. A field
/// hands its text over on losing focus, and a press moves no focus.
fn take_typed_on_press(
    press: On<Pointer<Press>>,
    focus: Res<InputFocus>,
    texts: Query<&ChildOf, With<EditableText>>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    let pressed = press.original_event_target();
    // The press rises through every node above the one it met.
    if press.event_target() != pressed {
        return;
    }
    let Some(input) = focus.get() else {
        return;
    };
    let Ok(field) = texts.get(input).map(ChildOf::parent) else {
        return;
    };
    let inside = pressed == field
        || parents
            .iter_ancestors(pressed)
            .any(|above| above == field);
    if inside {
        return;
    }
    commands.queue(move |world: &mut World| {
        // Told that it lost focus, the field hands its text over. It
        // has no focus for that moment, so that the chat's field
        // keeps its line and does not send it.
        world.resource_mut::<InputFocus>().clear();
        world.trigger(FocusLost { entity: input });
        world.flush();
        world
            .resource_mut::<InputFocus>()
            .set(input, FocusCause::Pressed);
    });
}

/// Lays a node out or takes it out of the layout, written only on a
/// difference.
pub fn show(
    node: &mut bevy::ecs::change_detection::Mut<Node>,
    shown: bool,
) {
    use bevy::ui::Display::{Flex, None};
    let want = if shown { Flex } else { None };
    if node.display != want {
        node.display = want;
    }
}

#[cfg(test)]
mod tests {
    use bevy::app::App;

    use super::*;

    #[test]
    fn a_press_outside_a_field_takes_what_was_typed_in_it_first() {
        use bevy::camera::NormalizedRenderTarget;
        use bevy::math::Vec2;
        use bevy::picking::backend::HitData;
        use bevy::picking::pointer::{
            Location, PointerButton, PointerId,
        };

        #[derive(Resource, Default)]
        struct Lost(Vec<(Entity, Option<Entity>)>);

        let mut app = App::new();
        app.init_resource::<InputFocus>()
            .init_resource::<Lost>()
            .add_observer(take_typed_on_press)
            .add_observer(
                |lost: On<FocusLost>,
                 focus: Res<InputFocus>,
                 mut seen: ResMut<Lost>| {
                    seen.0.push((lost.entity, focus.get()));
                },
            );
        let world = app.world_mut();
        let field = world.spawn_empty().id();
        let input = world
            .spawn((EditableText::default(), ChildOf(field)))
            .id();
        let button = world.spawn_empty().id();
        world
            .resource_mut::<InputFocus>()
            .set(input, FocusCause::Pressed);
        let mut press = |target: Entity| {
            let world = app.world_mut();
            let at = Location {
                target: NormalizedRenderTarget::None {
                    width: 1,
                    height: 1,
                },
                position: Vec2::ZERO,
            };
            let event = Press {
                button: PointerButton::Primary,
                hit: HitData::new(target, 0.0, None, None),
                count: 1,
            };
            world.trigger(Pointer::new(
                PointerId::Mouse,
                at,
                event,
                target,
            ));
            world.flush();
            std::mem::take(&mut world.resource_mut::<Lost>().0)
        };
        assert!(press(input).is_empty());
        assert!(press(field).is_empty());
        // The field hears of it with no focus on it, at once.
        assert_eq!(press(button).first(), Some(&(input, None)));
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(input)
        );
    }
}
