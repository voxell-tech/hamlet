//! The `profile` table: each profile the realm hires from, with its
//! kind and its name.

use anyhow::{Context as _, Result};
use hamlet_actor::BindParam;
use hamlet_messages::address::Profile;
use hamlet_messages::profile::ProfileInput;
use hamlet_messages::realm::ProfileId;
use hamlet_messages::realm::profiles::ProfileRefusal;
use serde::{Deserialize, Serialize};

use super::change::{self, Rows, Writer};
use super::settings;
use super::steps::Cx;

/// The character the Hand's profile starts from.
const HAND_CHARACTER: &str =
    include_str!("../../../hamlet_server/assets/hand.md");

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    /// The Hand's own.
    Hand,
    /// A blank hire's own, made for it alone (H38).
    Blank,
    /// One any hire may be made of.
    Shared,
}

struct Row {
    kind: Kind,
    name: String,
}

async fn row(
    rows: &Rows<'_>,
    profile: ProfileId,
) -> Result<Option<Row>> {
    let Some((kind, name)) = rows
        .row::<(String, String)>(
            "SELECT kind, name FROM profile WHERE id = ?1",
            vec![id(profile)],
        )
        .await?
    else {
        return Ok(None);
    };
    Ok(Some(Row {
        kind: change::from_case_name(kind)?,
        name,
    }))
}

fn id(profile: ProfileId) -> BindParam {
    BindParam::Integer(i64::from(profile.0))
}

/// How a chief and an agent are shown a profile:
/// `scholar#<id> (<name>)`.
pub(super) fn label(profile: ProfileId, name: &str) -> String {
    format!("scholar#{} ({name})", profile.0)
}

/// `named`, when a hire may be made of it: a shared profile.
pub(super) async fn hireable(
    rows: &Rows<'_>,
    named: ProfileId,
) -> Result<Result<ProfileId, ProfileRefusal>> {
    Ok(match row(rows, named).await? {
        Some(Row {
            kind: Kind::Shared, ..
        }) => Ok(named),
        Some(Row {
            kind: Kind::Blank,
            name,
        }) => Err(ProfileRefusal::Apart {
            label: label(named, &name),
        }),
        Some(Row {
            kind: Kind::Hand, ..
        })
        | None => {
            Err(ProfileRefusal::NoSuchProfile { profile: named })
        }
    })
}

pub(super) async fn insert(
    writer: &Writer<'_>,
    profile: ProfileId,
    kind: Kind,
    name: &str,
) -> Result<()> {
    writer
        .execute(
            "INSERT INTO profile (id, kind, name) VALUES (?1, ?2, \
             ?3)",
            vec![
                id(profile),
                BindParam::Text(change::case_name(&kind)?),
                BindParam::Text(name.to_owned()),
            ],
        )
        .await?;
    Ok(())
}

/// Makes `profile`'s actor from its character, or starts a shared
/// one, which was made with its own. A profile made already counts
/// as made.
pub(super) async fn start(
    cx: &Cx<'_>,
    profile: ProfileId,
) -> Result<()> {
    let rows = cx.rows()?;
    let kept = row(&rows, profile).await?.with_context(|| {
        format!("profile {} has no row", profile.0)
    })?;
    let text = match kept.kind {
        Kind::Shared => {
            return cx.peers().get_or_start(&Profile(profile)).await;
        }
        Kind::Hand => HAND_CHARACTER.to_owned(),
        Kind::Blank => settings::read(&rows).await?.character,
    };
    cx.peers()
        .create(&Profile(profile), &ProfileInput::Character { text })
        .await
}
