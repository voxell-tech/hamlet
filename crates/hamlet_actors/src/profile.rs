//! The `profile` actor, one per profile: the only writer of its
//! repository, `scholars/<n>.git`, and of the store beside it.

mod dreams;
mod files;
mod hires;
mod versions;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use hamlet_actor::{Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory};
use hamlet_git::{Change, MAIN};
use hamlet_host::{Caller, HostRef};
use hamlet_messages::Message;
use hamlet_messages::address::{self, Party, Realm};
use hamlet_messages::profile::{
    Branch, CommitId, DropSaveParts, Edit, EndDream, Erase, Files,
    Papers, Pin, PrepareDream, ProfileInput, Purge, Read, Revert,
    Save, Sha1, Tag, TooLong, Versions,
};
#[cfg(test)]
use hamlet_messages::realm::ProfileId;

use crate::repo::git::{By, Git};
use crate::repo::store::Store;
use crate::repo::{self, Admits, Owner, Repo, Settle, Takes};

/// The store's tables, each under the name of the one module that
/// writes it.
const SCHEMA: &str = include_str!("profile/schema.sql");

/// The folder of the profiles' repositories, under the realm folder.
const REPOSITORIES: &str = "scholars";

const CHARACTER: &str = "CHARACTER.md";

const CHARACTER_MOST: u32 = 4000;

/// The message of a profile's first commit.
const MADE: &str = "Made the scholar";

/// The factory of the `profile` kind.
pub fn factory() -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<Profile>(
        address::Profile::KIND,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
    registry
        .into_factories()
        .remove(address::Profile::KIND)
        .expect("the registry holds the kind it registered")
}

pub(crate) struct Profile {
    repo: Repo,
    own: address::Profile,
}

#[async_trait::async_trait]
impl Actor for Profile {
    type Input = Option<ProfileInput>;
    type Actions = (
        Branch,
        DropSaveParts,
        Edit,
        EndDream,
        Erase,
        Files,
        Papers,
        Pin,
        PrepareDream,
        Purge,
        Read,
        Revert,
        Save,
        Tag,
        Versions,
    );
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        let input = ctx
            .input()?
            .context("a profile is created with its input")?;
        if let ProfileInput::Character { text } = &input {
            let most = CHARACTER_MOST;
            ensure!(
                text.chars().count() <= most as usize,
                "{}",
                TooLong::CharacterTooLong { most }
            );
        }
        let own = address::Profile::own(ctx)?;
        let repositories =
            HostRef::of(ctx.inner())?.folder().join(REPOSITORIES);
        let path = repository(&repositories, own);
        let repo = Repo::start(
            ctx,
            SCHEMA,
            path,
            move |git| first_commit(git, &input, &repositories),
            |_| Ok(()),
        )
        .await?;
        Ok(Self { repo, own })
    }
}

fn repository(
    repositories: &Path,
    profile: address::Profile,
) -> PathBuf {
    repositories.join(format!("{}.git", profile.0.0))
}

fn first_commit(
    git: &Git<'_>,
    input: &ProfileInput,
    repositories: &Path,
) -> Result<()> {
    match input {
        ProfileInput::Character { text } => {
            let character = Change::Write {
                path: CHARACTER.to_owned(),
                bytes: text.clone().into_bytes(),
            };
            git.commit(MAIN, None, &[character], MADE, By::Realm)?;
        }
        ProfileInput::Fork { profile, at } => {
            let source =
                repository(repositories, address::Profile(*profile));
            git.start_from(
                MAIN,
                &source,
                git_commit(*at)?,
                "Forked",
                By::Realm,
            )?;
        }
    }
    Ok(())
}

impl<M> Handler<M> for Profile
where
    M: Message<To = address::Profile>,
    M::From: Send + Sync + 'static,
    Profile: Takes<M>,
{
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        message: M,
    ) -> Result<M::Output> {
        repo::admit(self, ctx, message).await
    }
}

impl Owner for Profile {
    type Party = address::Profile;

    fn repo(&self) -> &Repo {
        &self.repo
    }

    fn settle(settle: &mut Settle<'_, Self>) -> Result<()> {
        hires::follow_main(settle)
    }
}

impl Admits<Realm> for Profile {
    type Known = ();

    fn known(_: &Store<'_>, _: &Realm) -> Result<Option<()>> {
        Ok(Some(()))
    }
}

/// A commit as a message names it.
fn commit_id(commit: hamlet_git::CommitId) -> Result<CommitId> {
    Sha1::from_hex(&commit.to_string())
        .map(CommitId)
        .context("a commit's id is a SHA-1")
}

/// The commit a message names.
fn git_commit(commit: CommitId) -> Result<hamlet_git::CommitId> {
    commit.0.to_string().parse().map_err(anyhow::Error::msg)
}

#[cfg(test)]
impl repo::testing::Hostless {
    pub(crate) fn profile(home: &Path, character: &str) -> Self {
        Self::start_profile(home, character)
            .expect("a scratch folder takes a repository")
    }

    pub(crate) fn start_profile(
        home: &Path,
        character: &str,
    ) -> Result<Self> {
        let input = ProfileInput::Character {
            text: character.to_owned(),
        };
        let repositories = home.join(REPOSITORIES);
        let path =
            repository(&repositories, address::Profile(ProfileId(1)));
        Self::new(home, path, SCHEMA, |git| {
            first_commit(git, &input, &repositories)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};
    use std::time::Duration;

    use hamlet_actor::ActionSet;
    use hamlet_actor_error::ActorError;
    use hamlet_host::{Host, HostOptions};
    use hamlet_messages::profile::{
        By, DreamId, DreamOf, Revision, VersionName,
    };
    use hamlet_messages::workspace::{AgentNumber, CallId};
    use hamlet_protocol::client::RequestId;
    use hamlet_testing::Scratch;

    use super::*;

    fn args<M: Message>(message: &M) -> (&'static str, Vec<u8>) {
        (
            M::NAME,
            hamlet_actor::action::encode_positional(message).unwrap(),
        )
    }

    fn every_action() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            args(&Branch {
                name: "hires/7".into(),
                from: None,
            }),
            args(&DropSaveParts {
                agent: AgentNumber(7),
            }),
            args(&Tag {
                id: RequestId::new(),
                version: VersionName("v1".into()),
                at: None,
                note: String::new(),
                by: By::Chief { chief: 3 },
            }),
            args(&Versions),
            args(&Files {
                since: None,
                after: None,
            }),
            args(&Save {
                call: CallId("c".into()),
                base: CommitId(Sha1([0; 20])),
                part: 0,
                last: true,
                changes: Vec::new(),
            }),
            args(&Pin {
                id: RequestId::new(),
                hire: AgentNumber(7),
                to: None,
                by: By::Chief { chief: 3 },
            }),
            args(&Revert {
                id: RequestId::new(),
                what: Revision::Version {
                    version: VersionName("v1".into()),
                },
                reason: None,
                by: By::Chief { chief: 3 },
            }),
            args(&Edit {
                id: RequestId::new(),
                path: "memory/a.md".into(),
                text: "a".into(),
                message: None,
                by: By::Chief { chief: 3 },
            }),
            args(&Purge {
                id: RequestId::new(),
                path: "memory/a.md".into(),
                by: By::Chief { chief: 3 },
            }),
            args(&Papers),
            args(&Read {
                at: None,
                path: None,
            }),
            args(&Erase {
                id: RequestId::new(),
            }),
            args(&PrepareDream {
                dream: DreamId(1),
                of: DreamOf::Profile,
            }),
            args(&EndDream {
                dream: DreamId(1),
                done: true,
                summary: String::new(),
            }),
        ]
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn every_action_refuses_a_caller_it_does_not_admit() {
        let actions = every_action();
        let listed = actions
            .iter()
            .map(|(name, _)| *name)
            .collect::<BTreeSet<_>>();
        let taken = <<Profile as Actor>::Actions as ActionSet<
            Profile,
        >>::entries()
        .into_iter()
        .map(|entry| entry.name)
        .collect::<BTreeSet<_>>();
        assert_eq!(listed, taken, "one call for each action");

        let dir = Scratch::new("profile-every-action-caller");
        let host = Host::open_with(
            &dir,
            HashMap::from([(
                address::Profile::KIND.to_owned(),
                factory(),
            )]),
            HostOptions::default(),
        )
        .unwrap();
        let profile =
            address::Profile(ProfileId(1)).address().unwrap();
        let mut input = Vec::new();
        ciborium::into_writer(
            &Some(ProfileInput::Character {
                text: "A careful reader.".into(),
            }),
            &mut input,
        )
        .unwrap();
        host.create(&profile, input).await.unwrap();
        for (name, args) in actions {
            let error = host
                .call(&profile, name, args, Duration::from_secs(20))
                .await
                .expect_err(name);
            let error = ActorError::extract(&error);
            assert_eq!(
                format!("{}.{}", error.group(), error.code()),
                "message.wrong_caller",
                "{name}"
            );
        }
        host.shutdown().await.unwrap();
    }
}
