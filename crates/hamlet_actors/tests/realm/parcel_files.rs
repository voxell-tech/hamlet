//! A workspace that holds files for parcels, stood in for, and the
//! realm around it.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use hamlet_actor::{Actor, Ctx, Handler};
use hamlet_actors::realm;
use hamlet_host::{Caller, Host, HostOptions, HostRef};
use hamlet_messages::address::{self, Party};
use hamlet_messages::bytes::{Base64, Sha256};
use hamlet_messages::realm::letters::Enclosed;
use hamlet_messages::realm::parcels::{
    PARCEL_PART, Part, part_start,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::refusal::{PathRefusal, ToRead};
use hamlet_messages::workspace::{
    AnswerTaken, ChiefAnswer, InForce, Parcel, ParcelAnswer,
    ParcelEntry, ParcelFault, ParcelPart, ParcelRefusal, SetReach,
};
use hamlet_testing::Scratch;
use sha2::Digest as _;
use tokio::sync::watch;

use super::profile_stand_in::ProfileStandIn;
use super::stand_ins::{
    AgentStandIn, Callee, Forward, Seen, forward, kind, seen,
};
use super::{MAYOR, Realm, THREE, founded, hires, until};

/// The files the stand-in workspace holds, by path, and the parts it
/// holds back while a test asks.
#[derive(Default)]
pub(super) struct Files {
    held: Mutex<BTreeMap<PathBuf, Vec<u8>>>,
    /// Parts from this one on wait until it is cleared.
    pub(super) hold_from: watch::Sender<Option<u32>>,
    pub(super) waiting: Mutex<bool>,
}

impl Files {
    pub(super) fn with(files: &[(&str, &[u8])]) -> Self {
        let made = Self::default();
        made.add(files);
        made
    }

    pub(super) fn add(&self, files: &[(&str, &[u8])]) {
        let mut held = self.held.lock().unwrap();
        for (path, bytes) in files {
            held.insert(PathBuf::from(path), bytes.to_vec());
        }
    }

    /// What `paths` name, as the real workspace measures them: a
    /// path outside `/project` is out of reach.
    fn measure(&self, paths: &[Enclosed]) -> ParcelAnswer {
        let held = self.held.lock().unwrap();
        let mut files = Vec::new();
        for Enclosed { path: named } in paths {
            if !named.starts_with("/project") {
                let outside = PathRefusal::OutsideReach {
                    path: named.clone(),
                    access: ToRead::Read,
                };
                return ParcelAnswer::Refused {
                    why: ParcelFault::Unreadable {
                        path: named.clone(),
                        words: outside.to_string(),
                    }
                    .into(),
                };
            }
            let from = named.parent().unwrap();
            for (path, bytes) in held.range(named.clone()..) {
                if !path.starts_with(named) {
                    break;
                }
                files.push(ParcelEntry {
                    path: path.clone(),
                    name: path.strip_prefix(from).unwrap().to_owned(),
                    size: bytes.len() as u64,
                    sha256: hash(bytes),
                });
            }
        }
        if files.is_empty() {
            return ParcelAnswer::Refused {
                why: ParcelFault::NoFiles.into(),
            };
        }
        ParcelAnswer::Measured { files }
    }

    /// Part `part` of the file at `path`, as the real workspace
    /// reads it: a part past the file's end is refused.
    fn part(&self, path: &Path, part: u32) -> Part<ParcelRefusal> {
        let held = self.held.lock().unwrap();
        let bytes = &held[path];
        let Some(start) = part_start(bytes.len() as u64, part) else {
            return Part::Refused {
                why: ParcelFault::NoSuchPart {
                    path: path.to_owned(),
                    part,
                }
                .into(),
            };
        };
        let start = start as usize;
        let end = (start + PARCEL_PART).min(bytes.len());
        Part::Bytes {
            bytes: Base64(bytes[start..end].to_vec()),
            last: end == bytes.len(),
        }
    }

    pub(super) fn hold_from(&self, part: u32) {
        self.hold_from.send_replace(Some(part));
    }

    pub(super) fn release(&self) {
        self.hold_from.send_replace(None);
    }

    async fn called(&self, part: u32) {
        let mut held = self.hold_from.subscribe();
        let holds = |from: &Option<u32>| {
            from.is_some_and(|from| part >= from)
        };
        if holds(&held.borrow()) {
            *self.waiting.lock().unwrap() = true;
        }
        let _ = held.wait_for(|from| !holds(from)).await;
    }
}

pub(super) fn hash(bytes: &[u8]) -> Sha256 {
    Sha256(sha2::Sha256::digest(bytes).into())
}

/// Ages the store's file at `path` ten days, ten times the day the
/// store keeps a file no parcel names.
pub(super) fn aged(path: &Path) {
    let ten_days = Duration::from_secs(10 * 24 * 60 * 60);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - ten_days)
        .unwrap();
}

/// A workspace that takes a hire's reach and answers parcels from
/// [`Files`].
struct FilesStandIn {
    seen: Arc<Seen>,
    files: Arc<Files>,
}

#[async_trait::async_trait]
impl Actor for FilesStandIn {
    type Input = ();
    type Actions =
        (SetReach, ChiefAnswer, Parcel, ParcelPart, Forward);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self {
            seen: seen(ctx)?,
            files: HostRef::of(ctx.inner())?
                .value::<Files>()
                .expect("the host holds the workspace's files"),
        })
    }
}

impl Handler<SetReach> for FilesStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        set: SetReach,
    ) -> anyhow::Result<InForce> {
        let version = set.version;
        self.seen.reaches.lock().unwrap().push(set);
        Ok(InForce { version })
    }
}

impl Handler<ChiefAnswer> for FilesStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        answer: ChiefAnswer,
    ) -> anyhow::Result<AnswerTaken> {
        self.seen.chief_answers.lock().unwrap().push(answer);
        Ok(AnswerTaken::Taken)
    }
}

impl Handler<Parcel> for FilesStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        parcel: Parcel,
    ) -> anyhow::Result<ParcelAnswer> {
        assert_eq!(
            parcel.agent, MAYOR,
            "the sender's files are read"
        );
        self.seen.called(Callee::Parcel, parcel.agent).await?;
        Ok(self.files.measure(&parcel.paths))
    }
}

impl Handler<ParcelPart> for FilesStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        asked: ParcelPart,
    ) -> anyhow::Result<Part<ParcelRefusal>> {
        self.files.called(asked.part).await;
        Ok(self.files.part(&asked.path, asked.part))
    }
}

impl Handler<Forward> for FilesStandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Forward,
    ) -> anyhow::Result<Vec<u8>> {
        forward(&ctx, asked).await
    }
}

async fn open_with(
    dir: &Path,
    seen: &Arc<Seen>,
    files: &Arc<Files>,
) -> Host {
    let kinds = HashMap::from([
        (address::Realm::KIND.to_owned(), realm::factory()),
        (
            address::Agent::KIND.to_owned(),
            kind::<AgentStandIn>(address::Agent::KIND),
        ),
        (
            address::Profile::KIND.to_owned(),
            kind::<ProfileStandIn>(address::Profile::KIND),
        ),
        (
            address::Workspace::KIND.to_owned(),
            kind::<FilesStandIn>(address::Workspace::KIND),
        ),
    ]);
    let options = HostOptions::default()
        .with_value(seen.clone())
        .with_value(files.clone());
    let host = Host::open_with(dir, kinds, options).unwrap();
    for at in [address::Workspace.address(), address::Realm.address()]
    {
        host.get_or_start(&at.unwrap()).await.unwrap();
    }
    host
}

/// A realm whose workspace holds `files`, with mayor 2 and its hire
/// 3 live.
pub(super) async fn sending(files: Files) -> (Realm, Arc<Files>) {
    let scratch = Scratch::new("realm-parcels");
    let seen = Arc::new(Seen::default());
    let files = Arc::new(files);
    let host = open_with(&scratch, &seen, &files).await;
    let realm = Realm {
        host,
        seen,
        options: None,
        scratch,
    };
    realm.live(Hand::NUMBER).await;
    founded(&realm).await;
    realm.live(MAYOR).await;
    hires(&realm, MAYOR, "h0").await;
    realm.live(THREE).await;
    (realm, files)
}

pub(super) async fn restart(
    realm: Realm,
    files: &Arc<Files>,
) -> Realm {
    realm.host.shutdown().await.unwrap();
    let host = open_with(&realm.scratch, &realm.seen, files).await;
    Realm { host, ..realm }
}

/// Stops the realm's host while the workspace holds a part, then
/// lets the part go and opens the host again.
pub(super) async fn killed_while_held(
    realm: Realm,
    files: &Arc<Files>,
) -> Realm {
    let host = realm.host.clone();
    let down = tokio::spawn(async move { host.shutdown().await });
    let at = address::Realm.address().unwrap();
    until("the realm stops", || !realm.host.is_running(&at)).await;
    files.release();
    down.await.unwrap().unwrap();
    let host = open_with(&realm.scratch, &realm.seen, files).await;
    Realm { host, ..realm }
}
