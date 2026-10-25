//! The Agent Bazaar's panel: five stalls, each a tab, where the chief
//! adds models from OpenRouter and the endpoints to the catalogue,
//! takes skills from skills.sh into the library, and buys and sells
//! agents, tools and hamlets at the bazaar's server;
//! where that cannot be reached, those stalls show samples, and
//! hamlets are founded from their templates. A stall clicked in the
//! world opens it here.

mod covers;
mod errand;
mod hamlets;
mod listings;
mod market;
mod models;
mod samples;
mod shelf;
mod skills;
mod view;

#[cfg(test)]
mod tests;

use bevy::app::{App, PreUpdate};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists_and_changed;
use bevy::ecs::world::World;
use bevy_fynix::dock::DockWindowKind;
pub use hamlet_realm::bazaar::Stall;
use hamlet_realm::bazaar::Traded;
use hamlet_realm::pick::{Selected, Thing};
use hamlet_ui::styles::ICON;
use hamlet_ui::widgets::icons::Glyph;

use self::listings::{Agents, Hamlets, Tools, Wares};
use self::models::Ware;
use self::samples::agents::Hireling;
use self::samples::hamlets::Template;
use self::samples::plugins::Contraption;
use self::shelf::{Listing, Shelf, Stock};
use self::skills::skills_sh::Scroll;
use self::view::Shown;
use crate::PanelSystems;
use crate::dock::{PanelAppExt, open_panel, panel};

/// The bazaar's panel in the dock.
const PANEL: &str = "bazaar";

pub fn plugin(app: &mut App) {
    app.add_panel(
        PANEL,
        DockWindowKind::new("AGENT BAZAAR", || panel(view::screen()))
            .icon(Glyph::Coins.image(ICON)),
    )
    .init_resource::<StallShown>()
    .init_resource::<BazaarSettings>()
    .add_plugins((
        errand::plugin,
        models::plugin,
        skills::plugin,
        samples::plugin,
        hamlets::plugin,
        market::plugin,
        covers::plugin,
        view::plugin,
    ))
    .add_systems(
        PreUpdate,
        follow_selection
            .run_if(resource_exists_and_changed::<Selected>)
            .in_set(PanelSystems::Follow),
    );
}

/// How the bazaar's panel is drawn.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BazaarSettings {
    /// The rows a shelf shows at first, and the more each Show More
    /// adds.
    pub page: usize,
    /// The most pages of rows shown at once.
    pub pages: usize,
    /// The most characters of a SKILL.md shown on its page.
    pub read_most: usize,
    /// The height of a hamlet template's turning town on its page,
    /// in pixels.
    pub diorama: f32,
    /// Seconds the world is given to raise a hamlet founded from a
    /// template before the camera stops waiting to show it.
    pub seek: f32,
}

/// The stall the panel shows.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StallShown(pub Stall);

/// Opens the bazaar's panel at the stall it showed last.
pub(crate) fn open(world: &mut World) {
    open_panel(world, PANEL);
    let stall = world.resource::<StallShown>().0;
    stocked(world, stall);
}

/// Opens the bazaar's panel at `stall`.
pub(crate) fn show(world: &mut World, stall: Stall) {
    if world.resource::<StallShown>().0 != stall {
        world.resource_mut::<StallShown>().0 = stall;
    }
    open(world);
}

/// Opens the bazaar at OpenRouter's models.
pub(crate) fn browse_models(world: &mut World) {
    models::browse(world);
    show(world, Stall::Models);
}

/// Opens the bazaar at its hamlet templates.
pub(crate) fn browse_hamlets(world: &mut World) {
    show(world, Stall::Hamlets);
}

/// Opens the seller's desk at the stall shown, or at the agents.
pub(crate) fn sell(world: &mut World) {
    let stall = match world.resource::<StallShown>().0 {
        stall @ (Stall::Community
        | Stall::Plugins
        | Stall::Hamlets) => stall,
        _ => Stall::Community,
    };
    show(world, stall);
    view::sell(world, stall);
}

/// Asks for what `stall` sells the first time it is shown, and for
/// the page open there what that page shows.
pub(crate) fn stocked(world: &mut World, stall: Stall) {
    match stall {
        Stall::Models => {
            if !world.resource::<Stock<Ware>>().asked {
                models::fetch_wares(world);
            }
            reopen::<Ware>(world);
        }
        Stall::Skills => {
            if !world.resource::<Stock<Scroll>>().asked {
                skills::fetch_board(world);
            }
            reopen::<Scroll>(world);
        }
        Stall::Community => {
            market::stocked(world);
            reopen::<Hireling>(world);
            reopen::<Wares<Agents>>(world);
        }
        Stall::Plugins => {
            market::stocked(world);
            reopen::<Contraption>(world);
            reopen::<Wares<Tools>>(world);
        }
        Stall::Hamlets => {
            market::stocked(world);
            reopen::<Template>(world);
            reopen::<Wares<Hamlets>>(world);
        }
        Stall::Assets => {}
    }
}

/// Asks again for what the page open at the stall of `L` shows.
fn reopen<L: Listing + Shown>(world: &mut World) {
    if let Some(id) = world.resource::<Shelf<L>>().open.clone() {
        L::opened(world, &id);
    }
}

/// What OpenRouter and skills.sh answer, as their JSON bodies, for a
/// panel drawn with no network to ask.
pub struct Offered {
    /// OpenRouter's first page of models.
    pub models: serde_json::Value,
    /// skills.sh's first page of its leaderboard.
    pub skills: serde_json::Value,
    /// The files of skills, by their `owner/repo/skill`.
    pub files: Vec<(String, serde_json::Value)>,
}

/// Stocks the Models and Skills stalls with `offered`, as if it had
/// just been fetched.
pub fn offer(world: &mut World, offered: Offered) {
    let body = |value: serde_json::Value| {
        Ok((200, value.to_string().into_bytes()))
    };
    world.resource_mut::<Stock<Ware>>().ask();
    models::came(world, 0, body(offered.models));
    world.resource_mut::<Stock<Scroll>>().ask();
    skills::board(world, 0, body(offered.skills));
    for (path, files) in offered.files {
        skills::unrolled(world, skills::SkillId(path), body(files));
    }
}

pub use self::market::{Dealt, Staged, stage};

/// Opens the page of what the chief sells and has bought.
pub fn yours(world: &mut World) {
    view::yours(world);
}

/// Cheers a purchase in the world's bazaar.
pub(crate) fn traded(world: &mut World) {
    world.trigger(Traded);
}

/// Opens the bazaar at a stall chosen in the world, or as it was
/// when the bazaar is.
fn follow_selection(world: &mut World) {
    match world.resource::<Selected>().0 {
        Some(Thing::Stall(stall)) => show(world, stall),
        Some(Thing::Bazaar) => open(world),
        _ => {}
    }
}
