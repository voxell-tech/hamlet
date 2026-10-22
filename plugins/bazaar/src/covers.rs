//! The covers of the bazaar's listings: fetched from its server for
//! the listing whose page is open, one at a time, kept while it is
//! shown, and handed to the views no more than a call has room for.
//! And the cover chosen at the seller's desk, fetched from the
//! address given.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool, AtomicU64, AtomicUsize, Ordering,
};

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::{Failure, FetchReply, Reply, warn};

use crate::errand::{Errand, ErrandWorldExt};
use crate::listings::{Agents, Hamlets, Sells, Tools, Wares};
use crate::market::{self, Call, Market, Reach, client};
use crate::picture::{self, Header, Picture};
use crate::shelf::{Shelf, Stock, now};
use crate::view::{Desk, DeskPage};
use crate::{Stall, StallShown};

/// The most covers kept.
pub(crate) const MOST: usize = 24;

/// The most the covers kept may take shown, in bytes: with the cover
/// chosen at the desk and one shown twice as a view is built again,
/// well within the engine's `picture_memory` of 64 MiB.
pub(crate) const MEMORY: usize = 24 << 20;

/// The most covers fetched a minute, half the engine's
/// `fetches_per_minute`, leaving the rest to the stalls.
const A_MINUTE: u32 = 15;

const MINUTE: u64 = 60_000;

/// The bytes of pictures and requests handed to the engine in an
/// update past which no picture is: twice that, as a panel is
/// mounted, and the words of its views, are within a call's
/// `bytes_per_call` of 1 MiB, which the plugin is stopped past.
const AN_UPDATE: usize = picture::MOST_BYTES;

/// Where pictures may be fetched from at the desk, besides the
/// bazaar's server.
pub(crate) const PICTURE_HOST: &str = "raw.githubusercontent.com";

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Covers>()
        .init_resource::<CoverLine>()
        .init_resource::<Reveal>()
        .init_resource::<Pick>()
        .add_systems(First, refill)
        .add_systems(PostUpdate, (want, fetch).chain());
}

/// The covers kept, by their address on the server, which changes
/// with each new cover.
#[derive(Resource, Default)]
pub(crate) struct Covers {
    held: BTreeMap<String, Held>,
    /// The covers of the listings shown, first first.
    wanted: Vec<String>,
}

/// A cover fetched.
#[derive(Clone, Debug)]
enum Held {
    Shown(Picture),
    /// It may not be shown, so it is not fetched again while wanted.
    Refused,
    /// It did not fit in [`MEMORY`] with those wanted before it;
    /// fetched again once it does.
    Full(Header),
}

impl Covers {
    /// The cover at `url`, if it came and may be shown.
    pub fn get(&self, url: &str) -> Option<&Picture> {
        match self.held.get(url) {
            Some(Held::Shown(picture)) => Some(picture),
            _ => None,
        }
    }

    /// What the covers kept take shown, in bytes.
    pub fn memory(&self) -> usize {
        self.held
            .values()
            .map(|held| match held {
                Held::Shown(picture) => picture.header.memory(),
                _ => 0,
            })
            .sum()
    }

    /// Wants the covers at `urls`, the first first: those that fit in
    /// [`MEMORY`] in that order are kept or fetched again, and the
    /// rest let go of.
    pub fn want(&mut self, urls: Vec<String>) {
        self.held.retain(|url, _| urls.contains(url));
        let mut used = 0;
        for url in &urls {
            let (header, shown) = match self.held.get(url) {
                Some(Held::Shown(picture)) => (picture.header, true),
                Some(Held::Full(header)) => (*header, false),
                _ => continue,
            };
            if used + header.memory() <= MEMORY {
                used += header.memory();
                if !shown {
                    self.held.remove(url);
                }
            } else if shown {
                self.held.insert(url.clone(), Held::Full(header));
            }
        }
        self.wanted = urls;
    }

    /// The first cover wanted and not yet fetched, before any that
    /// did not fit.
    pub fn next(&self) -> Option<&String> {
        for url in &self.wanted {
            match self.held.get(url) {
                None => return Some(url),
                Some(Held::Full(_)) => return None,
                Some(_) => {}
            }
        }
        None
    }

    /// Keeps what came of fetching the cover at `url`, if it is still
    /// wanted: why it may not be shown, if it may not.
    pub fn came(
        &mut self,
        url: &str,
        bytes: Result<Vec<u8>, String>,
    ) -> Result<(), String> {
        if !self.wanted.iter().any(|wanted| wanted == url) {
            return Ok(());
        }
        let checked = bytes.and_then(|bytes| {
            picture::check(&bytes).map(|header| (bytes, header))
        });
        let (held, said) = match checked {
            Ok((bytes, header))
                if self.memory() + header.memory() <= MEMORY =>
            {
                (Held::Shown(Picture::new(bytes, header)), Ok(()))
            }
            Ok((_, header)) => (Held::Full(header), Ok(())),
            Err(why) => (Held::Refused, Err(why)),
        };
        self.held.insert(url.to_string(), held);
        said
    }
}

/// `urls` once each, the first [`MOST`] of them.
pub(crate) fn wanted(
    urls: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut all = Vec::new();
    for url in urls {
        if all.len() == MOST {
            break;
        }
        if !all.contains(&url) {
            all.push(url);
        }
    }
    all
}

/// The cover being fetched, and those fetched this minute.
#[derive(Resource, Default)]
pub(crate) struct CoverLine {
    out: Option<String>,
    /// When this minute began, in milliseconds since 1970.
    since: u64,
    count: u32,
}

impl CoverLine {
    fn allows(&self, now: u64) -> bool {
        self.out.is_none()
            && (now.saturating_sub(self.since) >= MINUTE
                || self.count < A_MINUTE)
    }

    fn sent(&mut self, url: String, now: u64) {
        if now.saturating_sub(self.since) >= MINUTE {
            self.since = now;
            self.count = 0;
        }
        self.count += 1;
        self.out = Some(url);
    }
}

/// The room left in this update for pictures handed to the views.
///
/// A view reads the world through a shared reference as it is built
/// or brought up to date, so what it takes of the room is counted in
/// atomics.
#[derive(Resource)]
pub(crate) struct Reveal {
    left: AtomicUsize,
    /// A picture waited for room, so the views read again next
    /// update.
    starved: AtomicBool,
}

impl Default for Reveal {
    fn default() -> Self {
        Reveal {
            left: AtomicUsize::new(AN_UPDATE),
            starved: AtomicBool::new(false),
        }
    }
}

impl Reveal {
    /// Counts `bytes` handed to the engine besides pictures.
    pub fn spend(&self, bytes: usize) {
        let _ = self.left.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |left| Some(left.saturating_sub(bytes)),
        );
    }

    /// What a view that last showed the picture numbered `shown`
    /// shows of `picture`: the picture once there is room for it,
    /// and nothing until then.
    pub fn show(
        &self,
        picture: Option<&Picture>,
        shown: &AtomicU64,
    ) -> Arc<[u8]> {
        let Some(picture) = picture else {
            shown.store(0, Ordering::Relaxed);
            return Arc::from([]);
        };
        if shown.load(Ordering::Relaxed) == picture.serial {
            return picture.bytes.clone();
        }
        let room = self.left.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |left| left.checked_sub(picture.bytes.len()),
        );
        match room {
            Ok(_) => {
                shown.store(picture.serial, Ordering::Relaxed);
                picture.bytes.clone()
            }
            Err(_) => {
                self.starved.store(true, Ordering::Relaxed);
                shown.store(0, Ordering::Relaxed);
                Arc::from([])
            }
        }
    }
}

fn refill(mut reveal: ResMut<Reveal>) {
    reveal.left.store(AN_UPDATE, Ordering::Relaxed);
    if reveal.starved.swap(false, Ordering::Relaxed) {
        reveal.set_changed();
    }
}

/// Wants the covers of the listings shown.
fn want(world: &mut World) {
    let urls = wanted(shown(world));
    if world.resource::<Covers>().wanted != urls {
        world.resource_mut::<Covers>().want(urls);
    }
}

/// The covers of the listings shown: that of the page open at the
/// stall of the bazaar's shown, or of the listing changed at the
/// desk.
fn shown(world: &World) -> Vec<String> {
    let at = match world.resource::<StallShown>().0 {
        Stall::Community => shown_at::<Agents>,
        Stall::Plugins => shown_at::<Tools>,
        Stall::Hamlets => shown_at::<Hamlets>,
        Stall::Models | Stall::Skills => return Vec::new(),
    };
    let market = world.resource::<Market>();
    let desk = world.resource::<Desk>();
    let urls = match desk.page {
        Some(DeskPage::Form) => desk
            .form
            .id
            .as_ref()
            .and_then(|id| market.lot(id))
            .and_then(|lot| lot.cover_url.clone())
            .into_iter()
            .collect(),
        Some(DeskPage::Yours) => Vec::new(),
        None if market.reach == Reach::Reached => at(world),
        None => Vec::new(),
    };
    urls.into_iter()
        .filter(|url| url.starts_with('/'))
        .collect()
}

fn shown_at<K: Sells>(world: &World) -> Vec<String> {
    world
        .resource::<Shelf<Wares<K>>>()
        .open
        .as_ref()
        .and_then(|id| world.resource::<Stock<Wares<K>>>().get(id))
        .and_then(|wares| wares.0.cover_url.clone())
        .into_iter()
        .collect()
}

/// Fetches the next cover wanted, when the line allows.
fn fetch(world: &mut World) {
    let now = now();
    if !world.resource::<CoverLine>().allows(now) {
        return;
    }
    let Some(url) = world.resource::<Covers>().next().cloned() else {
        return;
    };
    let request = world.resource::<Market>().server.picture(&url);
    world.resource_mut::<CoverLine>().sent(url.clone(), now);
    market::send(world, Call::Picture(url), request);
}

/// Keeps the cover fetched from `url`.
pub(crate) fn came(
    world: &mut World,
    url: String,
    reply: Result<Reply, Failure>,
) {
    let mut line = world.resource_mut::<CoverLine>();
    if line.out.as_deref() == Some(url.as_str()) {
        line.out = None;
    }
    let bytes = client::bytes(reply).map_err(|why| why.words());
    if let Err(why) = world.resource_mut::<Covers>().came(&url, bytes)
    {
        warn(&format!("the cover {url} is not shown: {why}"));
    }
}

/// The cover chosen at the desk, by the address it is fetched from.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(crate) struct Pick {
    pub url: String,
    pub state: Picked,
}

/// How fetching the cover chosen goes.
#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) enum Picked {
    #[default]
    Nothing,
    Fetching,
    Got(Picture),
    Failed(String),
}

/// Fetches the picture at `typed` as the cover chosen; none if it is
/// empty.
pub(crate) fn pick(world: &mut World, typed: String) {
    let url = typed.trim().to_string();
    let pick = world.resource::<Pick>();
    if pick.url == url && !matches!(pick.state, Picked::Failed(_)) {
        return;
    }
    let state = if url.is_empty() {
        Picked::Nothing
    } else if !url.starts_with("https://")
        && !url.starts_with("http://")
    {
        Picked::Failed(
            "An address begins with https:// or http://.".into(),
        )
    } else if world.send(url.clone(), Errand::Picture(url.clone())) {
        Picked::Fetching
    } else {
        Picked::Failed(
            "Hamlet held the fetch back for now. Try again in a \
             moment."
                .into(),
        )
    };
    *world.resource_mut::<Pick>() = Pick { url, state };
}

/// Takes in the picture fetched from `url` for the desk.
pub(crate) fn picked(
    world: &mut World,
    url: String,
    reply: Result<Reply, Failure>,
) {
    if world.resource::<Pick>().url != url {
        return;
    }
    let state = match reply {
        Ok(Reply::Fetched(FetchReply { status, body, .. }))
            if (200..300).contains(&status) =>
        {
            match picture::check(&body) {
                Ok(header) => Picked::Got(Picture::new(body, header)),
                Err(why) => Picked::Failed(why),
            }
        }
        Ok(Reply::Fetched(FetchReply { status, .. })) => {
            Picked::Failed(format!("It answered {status}."))
        }
        Ok(_) => Picked::Failed("It answered oddly.".into()),
        Err(Failure::Refused(why)) => Picked::Failed(format!(
            "Hamlet would not fetch it: {why}. The bazaar fetches \
             pictures only from {PICTURE_HOST} and its own server: \
             put the picture in a GitHub repository and give its \
             raw address."
        )),
        Err(why) => {
            Picked::Failed(format!("It could not be fetched: {why}."))
        }
    };
    world.resource_mut::<Pick>().state = state;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picture::tests::png;

    fn url(at: usize) -> String {
        format!("/v1/listings/l_{at}/cover?v=1")
    }

    #[test]
    fn covers_are_kept_while_wanted_and_within_their_memory() {
        let mut covers = Covers::default();
        covers.want(wanted((0..40).map(url)));
        assert_eq!(covers.wanted.len(), MOST);
        assert_eq!(covers.next(), Some(&url(0)));
        // 2048 by 1024 takes 8 MiB shown: three fill the memory.
        for at in 0..4 {
            covers.came(&url(at), Ok(png(2048, 1024))).unwrap();
        }
        assert!(covers.get(&url(2)).is_some());
        assert!(covers.get(&url(3)).is_none());
        assert_eq!(covers.memory(), MEMORY);
        assert!(covers.came(&url(4), Ok(b"junk".to_vec())).is_err());
        // Nothing more is fetched while the memory is full.
        assert_eq!(covers.next(), None);
        // One not wanted is not kept.
        covers.came(&url(30), Ok(png(8, 8))).unwrap();
        assert!(covers.get(&url(30)).is_none());

        // A cover that changed is another address, fetched again.
        let changed = url(0).replace("v=1", "v=2");
        covers.want(wanted([changed.clone(), url(1), url(3)]));
        assert!(covers.get(&url(0)).is_none());
        assert!(covers.get(&url(1)).is_some());
        assert_eq!(covers.memory(), 8 << 20);
        // Room was let go of, so the one that did not fit is asked
        // again.
        assert_eq!(covers.next(), Some(&changed));
        covers.came(&changed, Ok(png(16, 16))).unwrap();
        assert_eq!(covers.next(), Some(&url(3)));

        // Those wanted first are kept first.
        for at in [0, 2] {
            covers.want(wanted((0..4).map(url)));
            covers.came(&url(at), Ok(png(2048, 1024))).unwrap();
        }
        covers.came(&url(3), Ok(png(2048, 1024))).unwrap();
        assert_eq!(covers.next(), None);
        covers.want(wanted([3, 0, 1, 2].map(url)));
        assert_eq!(covers.next(), Some(&url(3)));
        assert!(covers.get(&url(1)).is_some());
        assert!(covers.get(&url(2)).is_none());
        covers.want(Vec::new());
        assert_eq!(covers.memory(), 0);
    }

    #[test]
    fn pictures_are_handed_over_within_an_update() {
        let reveal = Reveal::default();
        let big = |at| {
            let mut bytes = png(64, 64);
            bytes.resize(AN_UPDATE * 2 / 3, at);
            Picture::new(
                bytes,
                picture::header(&png(64, 64)).unwrap(),
            )
        };
        let (one, two) = (big(1), big(2));
        let (a, b) = (AtomicU64::new(0), AtomicU64::new(0));
        assert_eq!(
            reveal.show(Some(&one), &a).len(),
            one.bytes.len()
        );
        assert!(reveal.show(Some(&two), &b).is_empty());
        assert!(reveal.starved.load(Ordering::Relaxed));
        // Shown already, it is not counted again.
        assert_eq!(
            reveal.show(Some(&one), &a).len(),
            one.bytes.len()
        );

        let mut world = World::new();
        world.insert_resource(reveal);
        world.run_system_cached(refill).unwrap();
        let reveal = world.resource::<Reveal>();
        reveal.spend(AN_UPDATE / 2);
        assert!(reveal.show(Some(&two), &b).is_empty());
        world.run_system_cached(refill).unwrap();
        let reveal = world.resource::<Reveal>();
        assert_eq!(
            reveal.show(Some(&two), &b).len(),
            two.bytes.len()
        );
        assert!(reveal.show(None, &b).is_empty());
    }
}
