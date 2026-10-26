//! The bazaar's checks of what the chief sells, and reports of what
//! others sell: the chief's listings asked after while they are
//! checked, a rejected version handed to a person at the bazaar, and
//! a listing reported with its reason.

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Res;
use bevy::ecs::world::World;

use crate::bazaar::errand::Fetched;
use crate::bazaar::market::client::{done, read};
use crate::bazaar::market::lot::{self, Checked, Lot, Status};
use crate::bazaar::market::{Market, account, send};
use crate::bazaar::shelf::now;

/// How often the chief's listings are asked after while one is
/// checked, in milliseconds.
const EVERY: u64 = 5_000;

/// The most letters of a report's reason.
pub(crate) const REASON: usize = 1_000;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Watch>()
        .init_resource::<Reporting>()
        .add_systems(Update, watch.run_if(checking));
}

/// A call of the server about the checks or a report.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Call {
    /// A person asked to look again at the listing of this id.
    AskHuman(String),
    /// The listing of this id reported.
    Report(String),
}

/// When the chief's listings are next asked after, in milliseconds
/// since 1970.
#[derive(Resource, Default)]
pub(crate) struct Watch {
    next: u64,
}

/// A report of someone else's listing, as it is written and sent.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(crate) struct Reporting {
    /// The listing whose report is being written, if one is.
    pub open: Option<String>,
    pub reason: String,
    pub sending: bool,
    /// What came of the last report, of the listing of this id, and
    /// whether it went wrong.
    pub said: Option<(String, String, bool)>,
}

/// Whether `lot` waits for the bazaar's checks.
pub(crate) fn is_checked(lot: &Lot) -> bool {
    lot.status == Status::Pending
        || lot
            .check
            .as_ref()
            .is_some_and(|check| check.state == Checked::Pending)
}

fn checking(market: Res<Market>) -> bool {
    market.mine.iter().any(is_checked)
}

fn watch(world: &mut World) {
    let now = now();
    let mut watch = world.resource_mut::<Watch>();
    if now < watch.next {
        return;
    }
    watch.next = now + EVERY;
    account::ask_mine(world);
}

/// Asks a person at the bazaar to look again at the newest version of
/// the chief's listing `id`, which the checks rejected.
pub(crate) fn ask_human(world: &mut World, id: &str) {
    let market = world.resource::<Market>();
    let Some(version) = market
        .mine
        .iter()
        .find(|lot| lot.id == id)
        .and_then(|lot| lot.check.as_ref())
        .map(|check| check.version.clone())
    else {
        return;
    };
    let request = market.server.ask_human(id, &version);
    send(
        world,
        super::Call::Check(Call::AskHuman(id.to_string())),
        request,
    );
}

/// Opens the report of the listing `id`, or shuts it.
pub(crate) fn write_report(world: &mut World, id: Option<&str>) {
    let mut reporting = world.resource_mut::<Reporting>();
    reporting.open = id.map(str::to_string);
    reporting.reason.clear();
    reporting.said = None;
}

/// Sends the report being written, unless it says nothing.
pub(crate) fn report(world: &mut World) {
    let reporting = world.resource::<Reporting>();
    let (Some(id), false) =
        (reporting.open.clone(), reporting.sending)
    else {
        return;
    };
    let reason = reporting.reason.trim().to_string();
    if reason.is_empty() {
        world.resource_mut::<Reporting>().said =
            Some((id, "Say what is wrong with it.".into(), true));
        return;
    }
    world.resource_mut::<Reporting>().sending = true;
    let request =
        world.resource::<Market>().server.report(&id, &reason);
    send(world, super::Call::Check(Call::Report(id)), request);
}

pub(super) fn answered(
    world: &mut World,
    call: Call,
    answer: Fetched,
) {
    match call {
        Call::AskHuman(id) => match read::<lot::Check>(answer) {
            Ok(check) => {
                let mut market = world.resource_mut::<Market>();
                if let Some(lot) =
                    market.mine.iter_mut().find(|lot| lot.id == id)
                {
                    lot.check = Some(check);
                }
            }
            Err(why) => account::refused(world, why),
        },
        Call::Report(id) => {
            let said = match done(answer) {
                Ok(()) => (
                    "Thank you. A person at the bazaar will look at \
                     it."
                    .to_string(),
                    false,
                ),
                // Reported once already.
                Err(why) if why.status == Some(409) => (
                    "You reported it already. A person at the \
                     bazaar will look at it."
                        .to_string(),
                    false,
                ),
                Err(why) => (why.words(), true),
            };
            let mut reporting = world.resource_mut::<Reporting>();
            reporting.sending = false;
            if !said.1 {
                reporting.open = None;
                reporting.reason.clear();
            }
            reporting.said = Some((id, said.0, said.1));
        }
    }
}
