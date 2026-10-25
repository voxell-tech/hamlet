//! What agents and hamlets wear (H38): official assets of the Agent
//! Bazaar's server, kept in their hamlet's record. The server checks
//! only an asset's form; what the chief owns is known to that server
//! alone, and the window offers only what is owned.

use std::fmt;

use anyhow::Result;
use hamlet_protocol::client::{LooksSet, Wearer};
use thiserror::Error;

use crate::Outcome;
use crate::protocol::{Refusable, Refusal};
use crate::record::{Change, Looks, Wearer as Wears};
use crate::service::Service;

/// The longest asset name, `id@version`, kept.
const LONGEST: usize = 160;

/// A part of looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Skin,
    Mesh,
    Emote,
}

impl Part {
    fn word(self) -> &'static str {
        match self {
            Part::Skin => "skin",
            Part::Mesh => "mesh",
            Part::Emote => "emote",
        }
    }
}

impl fmt::Display for Part {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// Who wears looks: an agent's robot, or a hamlet's houses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Agent,
    Hamlet,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Agent => "agent",
            Kind::Hamlet => "hamlet",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Kind::Agent => "an agent",
            Kind::Hamlet => "a hamlet",
        })
    }
}

/// Looks that cannot be worn.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub(crate) enum Unworn {
    #[error(
        "`{0}` is not an official asset's id and version, such as \
         `agent-skin-sakura@1.0.0`."
    )]
    Malformed(String),
    #[error("`{asset}` is not a {part} for {kind}.")]
    Misfit {
        asset: String,
        part: Part,
        kind: Kind,
    },
}

impl Service {
    /// Puts on or takes off what `set` names for `wearer`. The same
    /// looks again change nothing.
    pub(crate) fn wear(
        &mut self,
        wearer: &Wearer,
        set: &LooksSet,
    ) -> Result<Refusable<String>> {
        let (hamlet, wears, kind, mut looks) = match wearer {
            Wearer::Agent { agent } => {
                let member =
                    match self.agent(agent, "change looks")? {
                        Ok(member) => member,
                        Err(error) => return Ok(Err(error)),
                    };
                let worn = self
                    .home
                    .store()
                    .record(&agent.hamlet)?
                    .and_then(|r| r.worn.get(&member.number).cloned())
                    .unwrap_or_default();
                (
                    &agent.hamlet,
                    Wears::Agent(member.number),
                    Kind::Agent,
                    worn,
                )
            }
            Wearer::Hamlet { hamlet } => {
                let record =
                    match self.access(hamlet, "change looks")? {
                        Ok(record) => record,
                        Err(error) => return Ok(Err(error)),
                    };
                (hamlet, Wears::Hamlet, Kind::Hamlet, record.looks)
            }
        };
        for (part, change, worn) in [
            (Part::Skin, &set.skin, &mut looks.skin),
            (Part::Mesh, &set.mesh, &mut looks.mesh),
            (Part::Emote, &set.emote, &mut looks.emote),
        ] {
            let Some(change) = change else { continue };
            if let Some(asset) = change
                && let Err(unworn) = fits(asset, kind, part)
            {
                return Ok(Err(Refusal::Unworn(unworn)));
            }
            worn.clone_from(change);
        }
        Ok(self
            .dress(hamlet, wears, looks)?
            .map(|()| "Saved.".into()))
    }

    fn dress(
        &mut self,
        hamlet: &str,
        wearer: Wears,
        looks: Looks,
    ) -> Result<Refusable<()>> {
        let operation = self.operation(
            hamlet,
            Change::SetLooks { wearer, looks },
            "",
        );
        let tx = self.home.store().transaction()?;
        if let Outcome::Refused(error) =
            crate::operation::change(&tx, &operation)?
        {
            return Ok(Err(Refusal::Record(Box::new(error))));
        }
        tx.commit()?;
        Ok(Ok(()))
    }
}

/// Whether `asset` is an official asset's `id@version` whose id,
/// `<kind>-<part>-<name>`, is a `part` for a `kind`.
fn fits(asset: &str, kind: Kind, part: Part) -> Result<(), Unworn> {
    let malformed = || Unworn::Malformed(asset.into());
    let (id, version) =
        asset.split_once('@').ok_or_else(malformed)?;
    let named = |text: &str, also: &[u8]| {
        !text.is_empty()
            && text.bytes().all(|b| {
                b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || also.contains(&b)
            })
    };
    let versioned = !version.is_empty()
        && version.bytes().all(|b| {
            b.is_ascii_alphanumeric() || b".+-".contains(&b)
        });
    if asset.len() > LONGEST || !named(id, b"-") || !versioned {
        return Err(malformed());
    }
    let mut words = id.splitn(3, '-');
    match (words.next(), words.next(), words.next()) {
        (Some(k), Some(p), Some(name))
            if k == kind.word()
                && p == part.word()
                && !name.is_empty() =>
        {
            Ok(())
        }
        (Some(_), Some(_), Some(name)) if !name.is_empty() => {
            Err(Unworn::Misfit {
                asset: asset.into(),
                part,
                kind,
            })
        }
        _ => Err(malformed()),
    }
}

#[cfg(test)]
mod tests {
    use hamlet_protocol::client::Address;
    use hamlet_testing::Scratch;

    use super::*;
    use crate::{Home, Overrides};

    fn set(
        skin: Option<Option<&str>>,
        emote: Option<Option<&str>>,
    ) -> LooksSet {
        let own = |part: Option<Option<&str>>| {
            part.map(|asset| asset.map(String::from))
        };
        LooksSet {
            skin: own(skin),
            mesh: None,
            emote: own(emote),
        }
    }

    #[test]
    fn looks_change_a_part_at_a_time_and_are_kept_in_the_record() {
        let scratch = Scratch::new("looks");
        let mut service = Service::new(
            Home::open(&scratch).unwrap(),
            Overrides::none(true),
        )
        .unwrap();
        let mayor = Wearer::Agent {
            agent: Address {
                hamlet: "home".into(),
                agent: "mayor".into(),
            },
        };
        let skin = "agent-skin-sakura@1.0.0";
        let wave = "agent-emote-wave@1.0.0";
        for change in [
            set(Some(Some(skin)), None),
            set(None, Some(Some(wave))),
            set(None, Some(Some(wave))),
        ] {
            let words = service.wear(&mayor, &change).unwrap();
            assert_eq!(words.unwrap(), "Saved.");
        }
        let worn = |service: &mut Service| {
            service.home.store().record("home").unwrap().unwrap()
        };
        let record = worn(&mut service);
        assert_eq!(
            record.worn.get(&record.mayor),
            Some(&Looks {
                skin: Some(skin.into()),
                mesh: None,
                emote: Some(wave.into()),
            })
        );

        // Taken off, the last part leaves the agent bare.
        service
            .wear(&mayor, &set(Some(None), Some(None)))
            .unwrap()
            .unwrap();
        assert!(worn(&mut service).worn.is_empty());

        let home = Wearer::Hamlet {
            hamlet: "home".into(),
        };
        let snow = "hamlet-skin-snow@1.0.0";
        service
            .wear(&home, &set(Some(Some(snow)), None))
            .unwrap()
            .unwrap();
        assert_eq!(
            worn(&mut service).looks.skin.as_deref(),
            Some(snow)
        );

        // A robot's skin does not fit a hamlet, and nothing changes.
        let refused =
            service.wear(&home, &set(Some(Some(skin)), None));
        assert!(matches!(
            refused.unwrap(),
            Err(Refusal::Unworn(Unworn::Misfit { .. }))
        ));
        assert_eq!(
            worn(&mut service).looks.skin.as_deref(),
            Some(snow)
        );
        let nowhere = Wearer::Hamlet {
            hamlet: "nowhere".into(),
        };
        assert!(
            service
                .wear(&nowhere, &set(None, None))
                .unwrap()
                .is_err()
        );
    }

    #[test]
    fn an_asset_fits_only_the_part_and_wearer_its_id_names() {
        assert_eq!(
            fits("agent-skin-sakura@1.0.0", Kind::Agent, Part::Skin),
            Ok(())
        );
        assert_eq!(
            fits(
                "hamlet-mesh-well-house@2.1.0",
                Kind::Hamlet,
                Part::Mesh
            ),
            Ok(())
        );
        for (asset, kind, part) in [
            ("agent-skin-sakura@1.0.0", Kind::Hamlet, Part::Skin),
            ("agent-skin-sakura@1.0.0", Kind::Agent, Part::Emote),
        ] {
            assert!(matches!(
                fits(asset, kind, part),
                Err(Unworn::Misfit { .. })
            ));
        }
        for asset in [
            "agent-skin-sakura",
            "agent-skin@1.0.0",
            "agent-skin-Sakura@1.0.0",
            "agent-skin-sakura@",
            "agent-skin-sakura@1 0",
            "../agent-skin-x@1.0.0",
        ] {
            assert_eq!(
                fits(asset, Kind::Agent, Part::Skin),
                Err(Unworn::Malformed(asset.into())),
                "{asset}"
            );
        }
    }
}
