//! What a profile's repository holds that the State shows, kept on
//! the profile's row: its row is read where the State is made, and a
//! profile is asked only after a call that can change it.

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::address::Profile;
use hamlet_messages::profile::{
    PaperList, Papers, VersionList, Versions,
};
use hamlet_messages::realm::ProfileId;
use serde::{Deserialize, Serialize};

use super::super::Realm;
use super::super::change::{self, Rows};
use super::super::peer::{Peers, When};
use super::{Change, id};

/// The names of the skills at `main`, sorted, and the versions.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub(in crate::realm) struct Holds {
    pub(in crate::realm) skills: Vec<String>,
    pub(in crate::realm) versions: Vec<Version>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(in crate::realm) struct Version {
    pub(in crate::realm) name: String,
    pub(in crate::realm) note: String,
    /// When it was made, in milliseconds since the epoch.
    pub(in crate::realm) at: i64,
}

impl Holds {
    fn of(papers: PaperList, versions: VersionList) -> Self {
        let mut skills = papers
            .skills
            .into_iter()
            .filter_map(|paper| {
                let rest = paper.path.strip_prefix("skills/")?;
                Some(rest.split_once('/')?.0.to_owned())
            })
            .collect::<Vec<_>>();
        skills.sort();
        skills.dedup();
        Self {
            skills,
            versions: versions
                .versions
                .into_iter()
                .map(|seen| Version {
                    name: seen.version.0,
                    note: seen.note,
                    at: seen.at,
                })
                .collect(),
        }
    }

    /// What the row of `profile` holds, none before its first read.
    pub(in crate::realm) async fn read(
        rows: &Rows<'_>,
        profile: ProfileId,
    ) -> Result<Self> {
        let kept = rows
            .value::<Option<Vec<u8>>>(
                "SELECT holds FROM profile WHERE id = ?1",
                vec![id(profile)],
            )
            .await?
            .flatten();
        Ok(match kept {
            Some(bytes) => ciborium::from_reader(bytes.as_slice())?,
            None => Self::default(),
        })
    }
}

/// The change that keeps what `profile`'s repository holds now, when
/// its row holds something else. A profile that does not answer
/// changes nothing, and is logged.
pub(in crate::realm) async fn reread(
    peers: &Peers,
    rows: &Rows<'_>,
    profile: ProfileId,
) -> Option<change::Change> {
    let read = async {
        let to = Profile(profile);
        let papers =
            peers.call(&to, &Papers, When::BeforeAnswer).await?;
        let versions =
            peers.call(&to, &Versions, When::BeforeAnswer).await?;
        let holds = Holds::of(papers, versions);
        Ok::<_, anyhow::Error>(
            (Holds::read(rows, profile).await? != holds)
                .then(|| Change::Holds { profile, holds }.into()),
        )
    };
    read.await.unwrap_or_else(|error| {
        tracing::warn!(
            profile = profile.0,
            "a profile's skills and versions were not read: \
             {error:#}"
        );
        None
    })
}

/// Keeps on its row what `profile`'s repository holds now. A failure
/// is logged: it never fails the call it follows.
pub(in crate::realm) async fn refresh(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    profile: ProfileId,
) {
    let kept = async {
        let rows = Rows::outside(ctx)?;
        if let Some(change) =
            reread(&realm.peers, &rows, profile).await
        {
            change::commit(realm, ctx, vec![change]).await?;
        }
        Ok::<_, anyhow::Error>(())
    };
    if let Err(error) = kept.await {
        tracing::warn!(
            profile = profile.0,
            "a profile's skills and versions were not kept: \
             {error:#}"
        );
    }
}
