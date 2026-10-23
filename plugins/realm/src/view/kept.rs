//! What the realm's view keeps between launches, the sky chosen, in
//! an asset of its own type in the engine's store, and the Realm page
//! of Settings that sets the quality.

use hamlet_engine_bevy::api::{AssetId, Reply, Ticket};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    ButtonStyle, FrameProps, Space, UiExt, button, column,
    description, keyed, label, resource, row,
};

use crate::world::sky::{Hour, Quality, SkyClock};

/// The type of the asset the view is kept in.
pub const KIND: &str = "realm.view";

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Kept>()
        .init_resource::<Store>()
        .add_settings_page(
            "graphics",
            "Graphics",
            "How richly the realm is drawn.",
            "sun",
            page,
        )
        .add_systems(Declare, find)
        .add_systems(
            Update,
            (answer, hold_sky, hold_quality, keep).chain(),
        );
}

/// The Graphics page: the quality the realm is drawn at.
fn page() -> AnyView {
    let choices =
        keyed(resource::<Kept, _>(|kept| kept.quality), |now| {
            let buttons = [
                (Quality::Low, "LOW"),
                (Quality::Medium, "MEDIUM"),
                (Quality::High, "HIGH"),
            ]
            .into_iter()
            .map(|(quality, words)| {
                button(words)
                    .style(if quality == *now {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Plain
                    })
                    .on_click(move |world| {
                        world.resource_mut::<Kept>().quality =
                            quality;
                    })
                    .boxed()
            })
            .collect::<Vec<_>>();
            row(buttons).space(Space::Half).boxed()
        });
    column((
        label("Quality"),
        choices,
        description(
            "Low has no particles and no bloom, and draws plain \
             edges and small shadows. Medium draws fewer particles \
             and middling shadows.",
        ),
    ))
    .space(Space::Half)
    .boxed()
}

/// The hour the sky shows, as the person chose it.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum SkyChoice {
    /// The chief's clock.
    #[default]
    Clock,
    Day,
    Night,
}

/// What the view keeps between launches.
#[derive(
    Resource,
    Clone,
    PartialEq,
    Debug,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(default)]
pub struct Kept {
    pub sky: SkyChoice,
    pub quality: Quality,
}

/// The asset [`Kept`] is in: the requests yet to be answered, and its
/// id once it is found or made. Nothing is written until the store
/// said whether there is one.
#[derive(Resource, Default)]
struct Store {
    asked: Vec<Ticket>,
    found: bool,
    id: Option<AssetId>,
    /// What was last written, or read.
    written: Option<Kept>,
}

/// Looks for the asset made at an earlier launch.
fn find(mut commands: Commands, mut store: ResMut<Store>) {
    let ticket = commands.list_assets(KIND);
    store.asked.push(ticket);
}

/// Takes in what the store answers: the asset found and read, or
/// made.
fn answer(
    mut commands: Commands,
    mut replies: MessageReader<Replied>,
    mut store: ResMut<Store>,
    mut kept: ResMut<Kept>,
) {
    for replied in replies.read() {
        let Some(at) =
            store.asked.iter().position(|t| *t == replied.ticket)
        else {
            continue;
        };
        store.asked.swap_remove(at);
        match &replied.reply {
            Ok(Reply::Listed(found)) => match found.first() {
                Some(info) => {
                    store.id = Some(info.id);
                    let ticket = commands.read_asset(info.id);
                    store.asked.push(ticket);
                }
                None => store.found = true,
            },
            Ok(Reply::Read(asset)) => {
                store.found = true;
                let text = String::from_utf8_lossy(&asset.bytes);
                match toml::from_str::<Kept>(&text) {
                    Ok(read) => {
                        store.written = Some(read.clone());
                        *kept = read;
                    }
                    Err(why) => hamlet_engine_bevy::warn(&format!(
                        "the realm's kept view: {why}"
                    )),
                }
            }
            Ok(Reply::Created(id)) => store.id = Some(*id),
            Ok(_) => {}
            Err(why) => {
                // A view that cannot be found is kept from now on.
                store.found = true;
                hamlet_engine_bevy::warn(&format!(
                    "the realm's kept view: {why}"
                ));
            }
        }
    }
}

/// Holds the sky at the hour chosen.
fn hold_sky(kept: Res<Kept>, mut sky: ResMut<SkyClock>) {
    let hour = match kept.sky {
        SkyChoice::Clock => Hour::Clock,
        SkyChoice::Day => Hour::Day,
        SkyChoice::Night => Hour::Night,
    };
    if sky.hour != hour {
        sky.hour = hour;
    }
}

/// Draws the realm at the quality chosen.
fn hold_quality(kept: Res<Kept>, mut quality: ResMut<Quality>) {
    if *quality != kept.quality {
        *quality = kept.quality;
    }
}

/// Writes what the view keeps once it changed, after the store
/// answered what was asked of it before.
fn keep(
    mut commands: Commands,
    kept: Res<Kept>,
    mut store: ResMut<Store>,
) {
    if !store.found
        || !store.asked.is_empty()
        || store.written.as_ref() == Some(&*kept)
    {
        return;
    }
    let text = match toml::to_string(&*kept) {
        Ok(text) => text,
        Err(why) => {
            hamlet_engine_bevy::warn(&format!(
                "the realm's view is not kept: {why}"
            ));
            return;
        }
    };
    let ticket = match store.id {
        Some(id) => commands.update_asset(id, text.as_bytes()),
        None => commands.create_asset(KIND, "View", text.as_bytes()),
    };
    store.asked.push(ticket);
    store.written = Some(kept.clone());
}
