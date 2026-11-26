//! A chief's acts on a profile's place in the city: its welcome, its
//! turning away and its move to another spot, and the newcomer wait
//! a welcome answers.

use anyhow::{Context as _, Result};
use hamlet_messages::realm::ProfileId;
use hamlet_messages::realm::client::Answer;
use hamlet_messages::realm::profiles::{
    LifeRefusal, Lifecycle, MoveHouse, ProfileRefusal, Site, Spot,
    TurnAway, Welcome,
};
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::sender::Client;
use hamlet_protocol::client::{Choice, HouseAt};

use super::super::super::act::{Act, Decided, Decision};
use super::super::super::calls::Made;
use super::super::super::change::{self, Rows};
use super::super::super::waits::answer::Ruling;
use super::super::super::waits::{self};
use super::super::super::{newcomers, record};
use super::super::{Arrival, Kind};
use super::{Found, living, refused};

pub(in crate::realm) struct Placing {
    found: Result<Found, ProfileRefusal>,
    /// The spots of every other house.
    taken: Vec<(f64, f64)>,
    /// The open newcomer waits about it.
    waits: Vec<WaitId>,
}

async fn placing(
    rows: &Rows<'_>,
    profile: ProfileId,
) -> Result<Placing> {
    Ok(Placing {
        found: living(rows, profile).await?,
        taken: super::super::houses(rows, Some(profile)).await?,
        waits: newcomers::waits_about(rows, profile).await?,
    })
}

/// The effect of a chief's answer to the newcomer wait `wait`: a
/// welcome lets its profile in as [`Welcome`] does, and a dismissal
/// leaves it at the harbor. A newcomer gone or let in since stays as
/// it is.
pub(in crate::realm) async fn newcomer_ruling(
    rows: &Rows<'_>,
    wait: WaitId,
    answer: &Answer,
) -> Result<Ruling> {
    if answer.0.choice != Choice::Welcome {
        return Ok(Ruling::default());
    }
    let detail = rows
        .value::<Vec<u8>>(
            "SELECT detail FROM wait WHERE id = ?1",
            vec![change::integer(wait.0)?],
        )
        .await?
        .with_context(|| format!("wait {} has no row", wait.0))?;
    let welcome = Welcome {
        profile: newcomers::of(&detail)?,
        why: answer
            .0
            .text
            .as_deref()
            .map(str::trim)
            .filter(|why| !why.is_empty())
            .map(str::to_owned),
    };
    let mut placing = welcome.load(&Client, rows).await?;
    placing.waits.retain(|other| *other != wait);
    Ok(match welcome.decide(&Client, placing) {
        Decision::Change(decided) => Ruling {
            changes: decided.changes,
            entries: decided.entries,
            ..Ruling::default()
        },
        _ => Ruling::default(),
    })
}

/// A living profile of the city's own: not a blank hire's.
fn of_the_city(
    found: Result<Found, ProfileRefusal>,
) -> Result<Found, Lifecycle> {
    let found = found.map_err(refused)?;
    if found.row.kind != Kind::Shared {
        return Err(refused(ProfileRefusal::Apart {
            label: found.label,
        }));
    }
    Ok(found)
}

/// The profile placed as `arrival`, with each of `closing` closed as
/// its choice.
fn placed(
    profile: ProfileId,
    label: String,
    arrival: Arrival,
    house: Option<HouseAt>,
    entry: record::Entry,
    closing: (Vec<WaitId>, waits::Choice),
) -> Decision<String, Lifecycle> {
    let (closed, choice) = closing;
    Decision::Change(
        Decided::new(label)
            .change(super::super::Change::Placed {
                profile,
                arrival,
                house,
                by: record::By::Chief,
            })
            .changes(closed.into_iter().map(|wait| {
                waits::Change::Close {
                    wait,
                    choice: choice.clone(),
                    text: None,
                }
                .into()
            }))
            .record(entry),
    )
}

impl Act<Client> for Welcome {
    type Output = Lifecycle;
    type Loaded = Placing;
    type Done = String;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Placing> {
        placing(rows, self.profile).await
    }

    fn decide(
        &self,
        _by: &Client,
        placing: Placing,
    ) -> Decision<String, Lifecycle> {
        let found = match of_the_city(placing.found) {
            Ok(found) => found,
            Err(answer) => return Decision::Answer(answer),
        };
        if found.row.arrival == Arrival::Housed {
            return Decision::Answer(refused(LifeRefusal::Housed {
                label: found.label,
            }));
        }
        let house = match super::super::free_spot(&placing.taken) {
            Ok(house) => house,
            Err(full) => {
                tracing::warn!("{full:#}");
                return Decision::Answer(refused(
                    LifeRefusal::Crowded { label: found.label },
                ));
            }
        };
        placed(
            self.profile,
            found.label,
            Arrival::Housed,
            Some(house),
            record::Entry::Welcomed {
                by: record::By::Chief,
                why: self.why.clone(),
            },
            (placing.waits, waits::Choice::Welcome),
        )
    }

    fn answer(&self, label: String, _made: &Made) -> Lifecycle {
        Lifecycle::Done { label }
    }
}

impl Act<Client> for TurnAway {
    type Output = Lifecycle;
    type Loaded = Placing;
    type Done = String;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Placing> {
        placing(rows, self.profile).await
    }

    fn decide(
        &self,
        _by: &Client,
        placing: Placing,
    ) -> Decision<String, Lifecycle> {
        let found = match of_the_city(placing.found) {
            Ok(found) => found,
            Err(answer) => return Decision::Answer(answer),
        };
        if found.row.arrival == Arrival::Turned {
            return Decision::Answer(refused(
                LifeRefusal::TurnedAway { label: found.label },
            ));
        }
        placed(
            self.profile,
            found.label,
            Arrival::Turned,
            None,
            record::Entry::TurnedAway {
                by: record::By::Chief,
                why: self.why.clone(),
            },
            (placing.waits, waits::Choice::Dismiss),
        )
    }

    fn answer(&self, label: String, _made: &Made) -> Lifecycle {
        Lifecycle::Done { label }
    }
}

impl Act<Client> for MoveHouse {
    type Output = Lifecycle;
    type Loaded = Placing;
    type Done = String;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Placing> {
        placing(rows, self.profile).await
    }

    fn decide(
        &self,
        _by: &Client,
        placing: Placing,
    ) -> Decision<String, Lifecycle> {
        let found = match of_the_city(placing.found) {
            Ok(found) => found,
            Err(answer) => return Decision::Answer(answer),
        };
        let spot = match &self.site {
            Site::Spot(spot) => Ok(*spot),
            Site::NotFinite { site } => {
                Err(LifeRefusal::BadSite { site: site.clone() })
            }
            Site::Unread => {
                return Decision::Answer(refused(
                    LifeRefusal::NoSpot { label: found.label },
                ));
            }
        };
        if found.row.arrival != Arrival::Housed {
            return Decision::Answer(refused(
                LifeRefusal::AtHarbor { label: found.label },
            ));
        }
        let Spot { x, z, degrees } = match spot {
            Ok(spot) => spot,
            Err(why) => return Decision::Answer(refused(why)),
        };
        if !HouseAt::is_clear(placing.taken, x, z) {
            return Decision::Answer(refused(LifeRefusal::Crowded {
                label: found.label,
            }));
        }
        let turn = degrees.map(f64::to_radians);
        placed(
            self.profile,
            found.label,
            Arrival::Housed,
            Some(super::super::house(x, z, turn)),
            record::Entry::HouseMoved {
                by: record::By::Chief,
            },
            (placing.waits, waits::Choice::Dismiss),
        )
    }

    fn answer(&self, label: String, _made: &Made) -> Lifecycle {
        Lifecycle::Done { label }
    }
}
