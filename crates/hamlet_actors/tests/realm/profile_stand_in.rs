//! A profile the realm calls, stood in for.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use anyhow::Context;
use hamlet_actor::{Actor, Ctx, Handler};
use hamlet_host::Caller;
use hamlet_messages::profile::{
    BadPath, Branch, Bundle, BundleAnswer, CommitId, DreamEnd,
    DreamOf, DreamOpen, DropBranch, DropSaveParts, Edit, EditRefusal,
    EndDream, Erase, MergeAnswer, MergeBranch, Missing,
    Path as FilePath, PrepareDream, Prepared, ProfileInput, Purge,
    PurgeAnswer, Read, ReadAnswer, Revert, RevertAnswer, Revision,
    Sha1, Source, Tag, TagAnswer, TagRefusal, Unmerge, UnmergeAnswer,
    VersionList, VersionName, VersionSeen, Versions,
};
use hamlet_messages::realm::{Done, ProfileId};
use hamlet_messages::workspace::AgentNumber;

use super::stand_ins::{Callee, Seen, key, seen};

pub(super) struct ProfileStandIn {
    seen: Arc<Seen>,
    profile: ProfileId,
}

#[async_trait::async_trait]
impl Actor for ProfileStandIn {
    /// `None` for a profile a test calls without making it.
    type Input = Option<ProfileInput>;
    type Actions = (
        Branch,
        Versions,
        Read,
        Edit,
        Tag,
        Revert,
        Purge,
        PrepareDream,
        EndDream,
        DropSaveParts,
        MergeBranch,
        Bundle,
        Unmerge,
        DropBranch,
        Erase,
    );
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self {
            seen: seen(ctx)?,
            profile: ProfileId(key(ctx)?),
        })
    }

    async fn on_create(
        self: Arc<Self>,
        ctx: Ctx<Self>,
    ) -> anyhow::Result<()> {
        if let Some(input) = ctx.input()? {
            self.seen
                .profiles
                .lock()
                .unwrap()
                .push((self.profile, input));
        }
        Ok(())
    }
}

const HELD_VERSION: &str = "v1";

impl ProfileStandIn {
    /// Whether `name` is its version or a branch it made.
    fn holds(&self, name: &str) -> bool {
        name == HELD_VERSION
            || self.seen.branches.lock().unwrap().iter().any(
                |(profile, branch)| {
                    *profile == self.profile && branch.name == name
                },
            )
    }
}

impl Handler<Branch> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        branch: Branch,
    ) -> anyhow::Result<Done<Missing>> {
        let agent = branch
            .name
            .strip_prefix("hires/")
            .context("a hire's branch is hires/<n>")?
            .parse()?;
        self.seen.called(Callee::Branch, AgentNumber(agent)).await?;
        let from = branch.from.clone();
        let held =
            from.as_ref().is_none_or(|from| self.holds(&from.0));
        self.seen
            .branches
            .lock()
            .unwrap()
            .push((self.profile, branch));
        Ok(match from {
            Some(version) if !held => Done::Refused {
                why: Missing::NoVersion {
                    profile: self.profile,
                    version,
                },
            },
            _ => Done::Done,
        })
    }
}

impl Handler<Versions> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Versions,
    ) -> anyhow::Result<VersionList> {
        Ok(VersionList {
            versions: vec![VersionSeen {
                version: VersionName(HELD_VERSION.to_owned()),
                commit: COMMIT,
                note: String::new(),
                by: "chief:1".to_owned(),
            }],
            commits: Vec::new(),
        })
    }
}

const HELD_FILE: &str = "CHARACTER.md";

impl Handler<Read> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        read: Read,
    ) -> anyhow::Result<ReadAnswer> {
        let lacked = match &read.at {
            Some(Revision::Version { version })
                if !self.holds(&version.0) =>
            {
                Some(version.clone())
            }
            _ => None,
        };
        let path = read.path.clone();
        self.seen.reads.lock().unwrap().push((self.profile, read));
        self.seen
            .called(Callee::Read, AgentNumber(self.profile.0))
            .await?;
        if let Some(version) = lacked {
            return Ok(ReadAnswer::Refused {
                why: Missing::NoVersion {
                    profile: self.profile,
                    version,
                }
                .into(),
            });
        }
        Ok(match path {
            None => ReadAnswer::Listed {
                paths: vec![FilePath {
                    path: HELD_FILE.to_owned(),
                }],
            },
            Some(path) if path == HELD_FILE => ReadAnswer::Text {
                text: format!(
                    "scholar#{}'s character",
                    self.profile.0
                ),
                cut: false,
            },
            Some(path) => ReadAnswer::Refused {
                why: BadPath::NotAPath { path }.into(),
            },
        })
    }
}

/// The commit every stand-in profile's change answers with.
const COMMIT: CommitId = CommitId(Sha1([7; 20]));

/// A path a stand-in profile refuses to write.
pub(super) const NOT_A_PATH: &str = "../out";

impl Handler<Edit> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        edit: Edit,
    ) -> anyhow::Result<Done<EditRefusal>> {
        let path = edit.path.clone();
        self.seen.edits.lock().unwrap().push((self.profile, edit));
        self.seen
            .called(Callee::Edit, AgentNumber(self.profile.0))
            .await?;
        Ok(if path == NOT_A_PATH {
            Done::Refused {
                why: BadPath::NotAPath { path }.into(),
            }
        } else {
            Done::Done
        })
    }
}

impl Handler<Tag> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        tag: Tag,
    ) -> anyhow::Result<TagAnswer> {
        let version = tag.version.clone();
        self.seen.tags.lock().unwrap().push((self.profile, tag));
        Ok(if version.0 == HELD_VERSION {
            TagAnswer::Refused {
                why: TagRefusal::Taken { version },
            }
        } else {
            TagAnswer::Tagged {
                version,
                commit: COMMIT,
            }
        })
    }
}

impl Handler<Revert> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        revert: Revert,
    ) -> anyhow::Result<RevertAnswer> {
        self.seen
            .reverts
            .lock()
            .unwrap()
            .push((self.profile, revert));
        Ok(RevertAnswer::Reverted {
            commit: COMMIT,
            clashes: vec![FilePath {
                path: "notes.md".to_owned(),
            }],
        })
    }
}

/// The hire every stand-in profile's purge leaves holding the path.
pub(super) const STILL_HOLDS: AgentNumber = AgentNumber(3);

impl Handler<Purge> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        purge: Purge,
    ) -> anyhow::Result<PurgeAnswer> {
        self.seen
            .called(Callee::Purge, AgentNumber(self.profile.0))
            .await?;
        let path = purge.path.clone();
        self.seen.purges.lock().unwrap().push((self.profile, purge));
        Ok(if path == NOT_A_PATH {
            PurgeAnswer::Refused {
                why: BadPath::NotAPath { path }.into(),
            }
        } else {
            PurgeAnswer::Purged {
                branches: 2,
                still_hold: vec![STILL_HOLDS],
            }
        })
    }
}

impl Handler<PrepareDream> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        prepare: PrepareDream,
    ) -> anyhow::Result<Prepared> {
        let purging = self
            .seen
            .waiting
            .lock()
            .unwrap()
            .iter()
            .any(|&(callee, _)| callee == Callee::Purge);
        self.seen
            .called(Callee::PrepareDream, AgentNumber(self.profile.0))
            .await?;
        let dream = prepare.dream.0;
        let of_a_hire = matches!(prepare.of, DreamOf::Hire { .. });
        self.seen.prepares.lock().unwrap().push((
            self.profile,
            prepare,
            purging,
        ));
        if self.seen.nothing_to_dream.load(Ordering::SeqCst) {
            return Ok(Prepared::Nothing);
        }
        Ok(Prepared::Ready {
            main: format!("dreams/{dream}/main"),
            hire: of_a_hire.then(|| format!("dreams/{dream}/hire")),
        })
    }
}

impl Handler<EndDream> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        end: EndDream,
    ) -> anyhow::Result<DreamEnd> {
        self.seen
            .called(Callee::EndDream, AgentNumber(self.profile.0))
            .await?;
        self.seen
            .dream_ends
            .lock()
            .unwrap()
            .push((self.profile, end));
        Ok(DreamEnd::Nothing)
    }
}

impl Handler<DropSaveParts> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        dropped: DropSaveParts,
    ) -> anyhow::Result<()> {
        self.seen
            .called(Callee::DropSaveParts, dropped.agent)
            .await?;
        self.seen
            .save_drops
            .lock()
            .unwrap()
            .push((self.profile, dropped));
        Ok(())
    }
}

impl ProfileStandIn {
    fn number(&self) -> AgentNumber {
        AgentNumber(self.profile.0)
    }
}

impl Handler<MergeBranch> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        merge: MergeBranch,
    ) -> anyhow::Result<MergeAnswer> {
        self.seen.called(Callee::Merge, merge.absorbed).await?;
        let kept =
            matches!(merge.from, Source::Part { last: false, .. });
        self.seen.merges.lock().unwrap().push((self.profile, merge));
        Ok(if kept {
            MergeAnswer::Kept
        } else {
            MergeAnswer::Merged {
                tip: COMMIT,
                set_aside: Vec::new(),
            }
        })
    }
}

impl Handler<Bundle> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        bundle: Bundle,
    ) -> anyhow::Result<BundleAnswer> {
        self.seen.called(Callee::Bundle, self.number()).await?;
        self.seen
            .bundles
            .lock()
            .unwrap()
            .push((self.profile, bundle));
        Ok(BundleAnswer::Part {
            files: Vec::new(),
            removed: Vec::new(),
            next: None,
        })
    }
}

impl Handler<Unmerge> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        unmerge: Unmerge,
    ) -> anyhow::Result<UnmergeAnswer> {
        self.seen.called(Callee::Unmerge, self.number()).await?;
        self.seen
            .unmerges
            .lock()
            .unwrap()
            .push((self.profile, unmerge));
        Ok(UnmergeAnswer::Unmerged { tip: COMMIT })
    }
}

impl Handler<DropBranch> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        drop: DropBranch,
    ) -> anyhow::Result<()> {
        let agent = drop
            .name
            .strip_prefix("hires/")
            .context("a hire's branch is hires/<n>")?
            .parse()?;
        self.seen
            .called(Callee::DropBranch, AgentNumber(agent))
            .await?;
        self.seen
            .branch_drops
            .lock()
            .unwrap()
            .push((self.profile, drop));
        Ok(())
    }
}

impl Handler<Erase> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Erase,
    ) -> anyhow::Result<Done<DreamOpen>> {
        self.seen
            .called(Callee::EraseProfile, self.number())
            .await?;
        self.seen.profile_erases.lock().unwrap().push(self.profile);
        Ok(Done::Done)
    }
}
