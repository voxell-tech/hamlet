//! A chief's acts on a hamlet: its name, its site, its mode, and its
//! folder with the `hamlet_folder` table of the folders it had.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use hamlet_actor::BindParam;
use hamlet_messages::realm::hamlets::{
    MoveHamlet, NameHamlet, SetFolder, SetMode,
};
use hamlet_messages::realm::{
    Done, HamletId, HamletRefusal, Refusable,
};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::reach::Mode;

use super::super::act::{Act, Decided, Decision};
use super::super::calls::Made;
use super::super::change::{self, Affected, Rows, Writer};
use super::super::record::{self, By};
use super::super::world::Part;

#[derive(Debug, PartialEq)]
pub(in crate::realm) enum Change {
    Name {
        hamlet: HamletId,
        name: String,
    },
    Site {
        hamlet: HamletId,
        x: f64,
        z: f64,
    },
    Mode {
        hamlet: HamletId,
        mode: Mode,
    },
    Folder {
        hamlet: HamletId,
        folder: Option<PathBuf>,
    },
}

pub(in crate::realm) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Name { hamlet, name } => {
            set(writer, &hamlet, "name = ?2", BindParam::Text(name))
                .await?;
            Ok(Affected::seen([Part::Hamlets]))
        }
        Change::Site { hamlet, x, z } => {
            writer
                .execute(
                    "UPDATE hamlet SET x = ?2, z = ?3 WHERE id = ?1",
                    vec![
                        BindParam::Text(hamlet.0),
                        BindParam::Float(x),
                        BindParam::Float(z),
                    ],
                )
                .await?;
            Ok(Affected::seen([Part::Hamlets]))
        }
        Change::Mode { hamlet, mode } => {
            let mode = BindParam::Text(change::case_name(&mode)?);
            set(writer, &hamlet, "mode = ?2", mode).await?;
            reached(&writer.rows(), &hamlet).await
        }
        Change::Folder { hamlet, folder } => {
            let folder = folder.as_deref().map(text).transpose()?;
            let bound = folder
                .clone()
                .map_or(BindParam::Null, BindParam::Text);
            set(writer, &hamlet, "folder = ?2", bound).await?;
            if let Some(folder) = folder {
                remember(writer, &hamlet, folder).await?;
            }
            let rows = writer.rows();
            Ok(Affected {
                places: hires_of(&rows, &hamlet).await?,
                ..reached(&rows, &hamlet).await?
            })
        }
    }
}

async fn set(
    writer: &Writer<'_>,
    hamlet: &HamletId,
    column: &str,
    value: BindParam,
) -> Result<()> {
    writer
        .execute(
            &format!("UPDATE hamlet SET {column} WHERE id = ?1"),
            vec![BindParam::Text(hamlet.0.clone()), value],
        )
        .await?;
    Ok(())
}

fn text(folder: &Path) -> Result<String> {
    folder.to_str().map(str::to_owned).with_context(|| {
        format!("the folder {} is not UTF-8", folder.display())
    })
}

/// Keeps `folder` among the folders `hamlet` had, once.
async fn remember(
    writer: &Writer<'_>,
    hamlet: &HamletId,
    folder: String,
) -> Result<()> {
    let params = vec![
        BindParam::Text(hamlet.0.clone()),
        BindParam::Text(folder),
        BindParam::Integer(writer.at().0),
    ];
    let kept = writer
        .execute(
            "UPDATE hamlet_folder SET at = ?3 WHERE hamlet = ?1 AND \
             folder = ?2",
            params.clone(),
        )
        .await?;
    if kept == 0 {
        writer
            .execute(
                "INSERT INTO hamlet_folder (hamlet, folder, at) \
                 VALUES (?1, ?2, ?3)",
                params,
            )
            .await?;
    }
    Ok(())
}

/// The reaches a change of the hamlet's footing moves: those of its
/// living hires and of their living workers.
async fn reached(
    rows: &Rows<'_>,
    hamlet: &HamletId,
) -> Result<Affected> {
    let reaches = rows
        .values::<u32>(
            "SELECT agent.id FROM agent LEFT JOIN hire ON \
             hire.agent = agent.id LEFT JOIN worker ON worker.agent \
             = agent.id WHERE agent.status IN ('hiring', 'live') \
             AND (hire.hamlet = ?1 OR worker.hamlet = ?1 OR \
             worker.starter IN (SELECT agent FROM hire WHERE hamlet \
             = ?1))",
            vec![BindParam::Text(hamlet.0.clone())],
        )
        .await?
        .into_iter()
        .map(AgentNumber)
        .collect();
    Ok(Affected {
        reaches,
        ..Affected::seen([Part::Hamlets, Part::Reaches])
    })
}

async fn hires_of(
    rows: &Rows<'_>,
    hamlet: &HamletId,
) -> Result<BTreeSet<AgentNumber>> {
    Ok(rows
        .values::<u32>(
            "SELECT hire.agent FROM hire JOIN agent ON agent.id = \
             hire.agent WHERE hire.hamlet = ?1 AND agent.status IN \
             ('hiring', 'live')",
            vec![BindParam::Text(hamlet.0.clone())],
        )
        .await?
        .into_iter()
        .map(AgentNumber)
        .collect())
}

async fn missing(
    rows: &Rows<'_>,
    hamlet: &HamletId,
) -> Result<Option<HamletRefusal>> {
    Ok(super::hamlet(rows, hamlet)
        .await?
        .is_none()
        .then(|| HamletRefusal::NoSuchHamlet { id: hamlet.clone() }))
}

/// The most characters a hamlet's name has.
const NAME_MOST: usize = 40;

/// `name` trimmed, when it is one line of 1 to [`NAME_MOST`]
/// characters with no colon.
pub(in crate::realm) fn checked_name(
    name: &str,
) -> Result<&str, HamletRefusal> {
    let name = name.trim();
    let shaped = (1..=NAME_MOST).contains(&name.chars().count())
        && !name.contains(char::is_control)
        && !name.contains(':');
    if shaped {
        Ok(name)
    } else {
        Err(HamletRefusal::BadName {
            name: name.to_owned(),
        })
    }
}

fn decided(
    refusal: Option<HamletRefusal>,
    change: Change,
    entry: record::Entry,
) -> Decision<(), Done<HamletRefusal>> {
    match refusal {
        Some(why) => Decision::Answer(Done::refused(why)),
        None => Decision::Change(
            Decided::new(()).change(change).record(entry),
        ),
    }
}

impl Act<Client> for NameHamlet {
    type Output = Done<HamletRefusal>;
    type Loaded = Option<HamletRefusal>;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<HamletRefusal>> {
        missing(rows, &self.hamlet).await
    }

    fn decide(
        &self,
        _by: &Client,
        refusal: Option<HamletRefusal>,
    ) -> Decision<(), Self::Output> {
        let refusal =
            refusal.or_else(|| checked_name(&self.name).err());
        let hamlet = self.hamlet.clone();
        decided(
            refusal,
            Change::Name {
                hamlet: hamlet.clone(),
                name: self.name.trim().to_owned(),
            },
            record::Entry::NamedHamlet {
                hamlet,
                by: By::Chief,
            },
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

impl Act<Client> for MoveHamlet {
    type Output = Done<HamletRefusal>;
    type Loaded = Option<HamletRefusal>;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<HamletRefusal>> {
        missing(rows, &self.hamlet).await
    }

    fn decide(
        &self,
        _by: &Client,
        refusal: Option<HamletRefusal>,
    ) -> Decision<(), Self::Output> {
        let is_site = self.x.is_finite() && self.z.is_finite();
        let refusal = refusal.or_else(|| {
            (!is_site).then(|| HamletRefusal::BadSite {
                site: format!("{}, {}", self.x, self.z),
            })
        });
        let hamlet = self.hamlet.clone();
        decided(
            refusal,
            Change::Site {
                hamlet: hamlet.clone(),
                x: self.x,
                z: self.z,
            },
            record::Entry::PlacedHamlet {
                hamlet,
                by: By::Chief,
            },
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

impl Act<Client> for SetMode {
    type Output = Done<HamletRefusal>;
    type Loaded = Option<HamletRefusal>;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<HamletRefusal>> {
        missing(rows, &self.hamlet).await
    }

    fn decide(
        &self,
        _by: &Client,
        refusal: Option<HamletRefusal>,
    ) -> Decision<(), Self::Output> {
        let hamlet = self.hamlet.clone();
        decided(
            refusal,
            Change::Mode {
                hamlet: hamlet.clone(),
                mode: self.mode,
            },
            record::Entry::Mode {
                hamlet,
                by: By::Chief,
            },
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

impl Act<Client> for SetFolder {
    type Output = Done<HamletRefusal>;
    type Loaded = Option<HamletRefusal>;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<HamletRefusal>> {
        if let Some(refusal) = missing(rows, &self.hamlet).await? {
            return Ok(Some(refusal));
        }
        let Some(folder) = &self.folder else {
            return Ok(None);
        };
        let is_folder = tokio::fs::metadata(folder)
            .await
            .is_ok_and(|found| found.is_dir());
        Ok((!is_folder).then(|| HamletRefusal::NoSuchFolder {
            path: folder.clone(),
        }))
    }

    fn decide(
        &self,
        _by: &Client,
        refusal: Option<HamletRefusal>,
    ) -> Decision<(), Self::Output> {
        let hamlet = self.hamlet.clone();
        decided(
            refusal,
            Change::Folder {
                hamlet: hamlet.clone(),
                folder: self.folder.clone(),
            },
            record::Entry::Folder {
                hamlet,
                by: By::Chief,
            },
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_that_is_no_number_is_refused() {
        // the host encodes no such number, so only the realm's own
        // callers can send one.
        let moved = MoveHamlet {
            hamlet: HamletId("h".to_owned()),
            x: f64::NAN,
            z: 4.0,
        };
        let Decision::Answer(Done::Refused { why }) =
            moved.decide(&Client, None)
        else {
            panic!("a site that is no number is refused");
        };
        assert_eq!(why.to_string(), "`NaN, 4` is not an address");
    }
}
