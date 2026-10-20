//! The typefaces, read from the repository's `assets/fonts/`, where
//! their licences are.

use bevy::app::{App, Startup, Update};
use bevy::asset::{AssetId, Assets, Handle};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::{Added, Changed, Or, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::text::{Font, FontSource, LetterSpacing, TextFont};

/// Inter (SIL Open Font License), the face of everything read.
const INTER: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/InterVariable.ttf"
));
/// Cinzel (SIL Open Font License), for titles.
const CINZEL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/CinzelVariable.ttf"
));

/// The spacing of the letters of a title.
const TRACKING: LetterSpacing = LetterSpacing::Px(1.0);

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, install)
        .add_systems(Update, apply_display);
}

/// The display face, for titles.
#[derive(Resource)]
pub struct Display(Handle<Font>);

/// A label drawn in the display face.
#[derive(Component, Clone, Copy)]
pub struct DisplayText;

/// Makes Inter the default font, so every label and field uses it.
fn install(world: &mut World) {
    // An app with no text, as a headless one, has no fonts to set.
    let Some(mut fonts) = world.get_resource_mut::<Assets<Font>>()
    else {
        return;
    };
    let _ = fonts
        .insert(AssetId::default(), Font::from_bytes(INTER.to_vec()));
    let display = fonts.add(Font::from_bytes(CINZEL.to_vec()));
    world.insert_resource(Display(display));
}

/// Sets the display face, tracked, on a [`DisplayText`] label when it
/// is new or was styled again.
fn apply_display(
    display: Option<Res<Display>>,
    mut labels: Query<
        (Entity, &mut TextFont, Option<&LetterSpacing>),
        (
            Or<(Changed<TextFont>, Added<DisplayText>)>,
            With<DisplayText>,
        ),
    >,
    mut commands: Commands,
) {
    let Some(display) = display else {
        return;
    };
    for (label, mut font, spacing) in &mut labels {
        if !matches!(&font.font, FontSource::Handle(h) if *h == display.0)
        {
            font.font = FontSource::Handle(display.0.clone());
        }
        if spacing != Some(&TRACKING) {
            commands.entity(label).try_insert(TRACKING);
        }
    }
}
