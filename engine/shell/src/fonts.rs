//! The typefaces, read from the repository's `assets/fonts/`, where
//! their licences are.

use bevy::prelude::*;
use bevy::text::{FontSize, FontSource, LetterSpacing};
use bevy_fynix::dock::DockTab;

use crate::theme::EngineTheme;

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

/// Sets the display face, tracked, on a [`DisplayText`] label and on
/// the name of a dock tab, when it is new or was styled again.
fn apply_display(
    display: Option<Res<Display>>,
    theme: Res<EngineTheme>,
    tabs: Query<(), With<DockTab>>,
    parents: Query<&ChildOf>,
    mut labels: Query<
        (
            Entity,
            &mut TextFont,
            Option<&LetterSpacing>,
            Has<DisplayText>,
        ),
        Or<(Changed<TextFont>, Added<DisplayText>)>,
    >,
    mut commands: Commands,
) {
    let Some(display) = display else {
        return;
    };
    let tracking = LetterSpacing::Px(theme.text.tracking);
    let tab_size = FontSize::Px(theme.text.small);
    for (label, mut font, spacing, tagged) in &mut labels {
        let tab = parents
            .iter_ancestors(label)
            .any(|above| tabs.contains(above));
        if !tagged && !tab {
            continue;
        }
        if !matches!(&font.font, FontSource::Handle(h) if *h == display.0)
        {
            font.font = FontSource::Handle(display.0.clone());
        }
        if tab && font.font_size != tab_size {
            font.font_size = tab_size;
        }
        if spacing != Some(&tracking) {
            commands.entity(label).try_insert(tracking);
        }
    }
}
