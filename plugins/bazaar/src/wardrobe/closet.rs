//! The closet: what the bazaar keeps in the engine's asset store, for
//! itself and for the realm (docs/looks.md). It keeps what is owned
//! and worn as a [`KEPT`] asset, downloads the file of each asset
//! worn and owned from the bazaar's server a part at a time, each
//! part a [`looks::FILE`] asset, writes the [`looks::WORN`] asset
//! once a file is whole, and then deletes the parts no `worn` names.
//! One write of the store is out at a time, well within the engine's
//! `asset_writes_per_second`.

use std::collections::BTreeMap;

use hamlet_engine_bevy::api::{AssetId, AssetInfo};
use hamlet_engine_bevy::bevy_ecs::message::MessageCursor;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::{Failure, Reply, Ticket, warn};
use looks::wardrobe::Target;
use looks::{FILE, FILE_PART, WORN, WORN_NAME, Worn, part_name};

use crate::covers::Reveal;
use crate::market::client::bytes;
use crate::market::{self, Market};
use crate::shelf::now;
use crate::wardrobe::turntable::Turning;
use crate::wardrobe::{Call, Kept, Wardrobe, emit_play};

/// The type of the asset that keeps what is owned and worn, made by
/// the bazaar alone, and the name of its one asset.
pub(crate) const KEPT: &str = "bazaar.wardrobe";
const KEPT_NAME: &str = "wardrobe";

/// The wait before a file that could not be downloaded is asked for
/// again, in milliseconds.
pub(crate) const RETRY: u64 = 30_000;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Closet>()
        .add_systems(Startup, look)
        .add_systems(PreUpdate, replied)
        .add_systems(Update, tend)
        .add_systems(Last, animate);
}

/// What the store holds of the closet's, as far as it is known, and
/// what is being done to it.
#[derive(Resource, Default)]
pub(crate) struct Closet {
    /// The store's requests asked as the plugin starts, not yet
    /// answered: nothing is written before they are.
    starting: u32,
    kept: Option<AssetId>,
    worn: Option<AssetId>,
    /// What the `worn` asset says, as last read or written.
    pub written: Worn,
    /// The files whole in the store, by name, with their parts.
    whole: BTreeMap<String, u32>,
    /// Every part in the store, by name.
    parts: BTreeMap<String, AssetId>,
    /// The store's requests out, by their tickets.
    asked: Vec<(Ticket, Asked)>,
    /// What is kept changed, and is to be written.
    keep: bool,
    pub download: Option<Download>,
    /// When each file that could not be downloaded was last asked
    /// for, in milliseconds since 1970.
    missed: BTreeMap<String, u64>,
    /// The agents whose emotes are played once the realm is told
    /// they wear one.
    play: Vec<String>,
    /// A request was not taken in its call, so the plugin is called
    /// again each frame until it is.
    stalled: bool,
}

/// A file being downloaded.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Download {
    pub file: String,
    /// The asset's id.
    id: String,
    /// The place of the next part asked for, or written.
    next: u32,
    /// The parts come and not yet written, first first.
    came: Vec<Vec<u8>>,
    /// The last part came.
    ended: bool,
}

/// A request of the store's, for its reply.
#[derive(Clone, PartialEq, Debug)]
enum Asked {
    List(&'static str),
    Read(&'static str),
    Make(&'static str, Option<Worn>),
    Write(&'static str, Option<Worn>),
    /// A part made or written, by its name.
    Part(String),
    Delete(String),
}

impl Asked {
    fn writes(&self) -> bool {
        !matches!(self, Asked::List(_) | Asked::Read(_))
    }
}

impl Closet {
    fn ready(&self) -> bool {
        self.starting == 0
    }

    fn writing(&self) -> bool {
        self.asked.iter().any(|(_, asked)| asked.writes())
    }

    /// Whether the file `file` is whole in the store.
    pub fn has(&self, file: &str) -> bool {
        self.whole.contains_key(file)
    }

    /// Whether the file `file` could not be downloaded lately.
    pub fn missed(&self, file: &str) -> bool {
        self.missed.contains_key(file)
    }

    /// The first part in the store that `worn` does not name, but
    /// those of the file `downloading`.
    fn unworn(
        &self,
        downloading: Option<&str>,
    ) -> Option<(String, AssetId)> {
        self.parts
            .iter()
            .find(|(name, _)| {
                let Some((file, at)) = split_part(name) else {
                    return true;
                };
                Some(file) != downloading
                    && self
                        .written
                        .parts
                        .get(file)
                        .is_none_or(|count| at >= *count)
            })
            .map(|(name, id)| (name.clone(), *id))
    }
}

/// The file and the place of the part named `name`.
pub(crate) fn split_part(name: &str) -> Option<(&str, u32)> {
    let (file, at) = name.rsplit_once('#')?;
    Some((file, at.parse().ok()?))
}

/// What part `at` of a file came as: the parts to write, and whether
/// it was the last. A part shorter than [`FILE_PART`] is the last; an
/// empty one past the first says the one before was. A server that
/// answers the whole file at once has it cut here.
pub(crate) fn cut(
    at: u32,
    bytes: Vec<u8>,
) -> Result<(Vec<Vec<u8>>, bool), String> {
    match bytes.len() {
        0 if at > 0 => Ok((Vec::new(), true)),
        len if len <= FILE_PART => Ok((vec![bytes], len < FILE_PART)),
        _ if at == 0 => Ok((
            bytes.chunks(FILE_PART).map(<[u8]>::to_vec).collect(),
            true,
        )),
        len => Err(format!(
            "{len} bytes came of part {at}, where {FILE_PART} at \
             most were asked for"
        )),
    }
}

fn look(world: &mut World) {
    for kind in [KEPT, WORN, FILE] {
        let ticket = world.list_assets(kind);
        starting(world, ticket, Asked::List(kind));
    }
    if world.resource::<Closet>().ready() {
        started(world);
    }
}

/// Remembers `asked` of `ticket`, one the start waits for.
fn starting(world: &mut World, ticket: Ticket, asked: Asked) {
    if ticket != 0 {
        let mut closet = world.resource_mut::<Closet>();
        closet.starting += 1;
        closet.asked.push((ticket, asked));
    }
}

/// Remembers `asked` of `ticket`; false when it was not taken.
fn remember(world: &mut World, ticket: Ticket, asked: Asked) -> bool {
    let mut closet = world.resource_mut::<Closet>();
    closet.stalled = ticket == 0;
    if ticket != 0 {
        closet.asked.push((ticket, asked));
    }
    ticket != 0
}

/// Has what is owned and worn kept for the next run.
pub(crate) fn keep(world: &mut World) {
    world.resource_mut::<Closet>().keep = true;
}

/// Has the file `file` asked for at once, if it could not be
/// downloaded.
pub(crate) fn retry(world: &mut World, file: &str) {
    if world.resource::<Closet>().missed.contains_key(file) {
        world.resource_mut::<Closet>().missed.remove(file);
    }
}

/// Has the agent `name` play its emote once the realm is told it
/// wears one.
pub(crate) fn play(world: &mut World, name: &str) {
    let mut closet = world.resource_mut::<Closet>();
    if !closet.play.iter().any(|one| one == name) {
        closet.play.push(name.to_string());
    }
}

fn replied(
    world: &mut World,
    mut replies: Local<MessageCursor<Replied>>,
) {
    let replied = replies
        .read(world.resource::<Messages<Replied>>())
        .cloned()
        .collect::<Vec<_>>();
    for Replied { ticket, reply } in replied {
        let Some(at) = world
            .resource::<Closet>()
            .asked
            .iter()
            .position(|(t, _)| *t == ticket)
        else {
            continue;
        };
        let mut closet = world.resource_mut::<Closet>();
        let (_, asked) = closet.asked.remove(at);
        let start = !closet.ready();
        match reply {
            Ok(reply) => took(world, asked, reply),
            Err(why) => failed(world, asked, &why.to_string()),
        }
        if start {
            let mut closet = world.resource_mut::<Closet>();
            closet.starting = closet.starting.saturating_sub(1);
            if closet.ready() {
                started(world);
            }
        }
    }
}

/// Takes in what the store answered `asked`.
fn took(world: &mut World, asked: Asked, reply: Reply) {
    match (asked, reply) {
        (Asked::List(kind), Reply::Listed(found)) => {
            listed(world, kind, found)
        }
        (Asked::Read(KEPT), Reply::Read(asset)) => {
            // What changed meanwhile is the newer.
            if world.resource::<Closet>().keep {
                return;
            }
            let kept = serde_json::from_slice::<Kept>(&asset.bytes)
                .unwrap_or_default();
            let mut wardrobe = world.resource_mut::<Wardrobe>();
            wardrobe.wearing = kept.wearing;
            if wardrobe.owned.is_empty() {
                wardrobe.owned = kept
                    .owned
                    .into_iter()
                    .map(|asset| (asset.id.clone(), asset))
                    .collect();
            }
        }
        (Asked::Read(_), Reply::Read(asset)) => {
            let worn = Worn::read(&asset.bytes).unwrap_or_default();
            world.resource_mut::<Closet>().written = worn;
        }
        (Asked::Make(kind, worn), Reply::Created(id)) => {
            let mut closet = world.resource_mut::<Closet>();
            match worn {
                Some(worn) => {
                    closet.worn = Some(id);
                    closet.written = worn;
                }
                None if kind == KEPT => closet.kept = Some(id),
                None => {}
            }
        }
        (Asked::Write(_, Some(worn)), Reply::Updated(_)) => {
            world.resource_mut::<Closet>().written = worn;
        }
        (Asked::Part(name), Reply::Created(id)) => {
            world.resource_mut::<Closet>().parts.insert(name, id);
            written(world);
        }
        (Asked::Part(_), Reply::Updated(_)) => written(world),
        (Asked::Delete(name), Reply::Deleted) => {
            world.resource_mut::<Closet>().parts.remove(&name);
        }
        _ => {}
    }
}

/// Takes in that the store refused `asked`.
fn failed(world: &mut World, asked: Asked, why: &str) {
    warn(&format!("the closet's {asked:?}: {why}"));
    let mut closet = world.resource_mut::<Closet>();
    match asked {
        Asked::Part(_) => {
            if let Some(download) = closet.download.take() {
                closet.missed.insert(download.file, now());
            }
        }
        // Gone already, or not to be deleted: not asked again.
        Asked::Delete(name) => {
            closet.parts.remove(&name);
        }
        // What is kept is written again on the next change.
        _ => {}
    }
}

fn listed(
    world: &mut World,
    kind: &'static str,
    found: Vec<AssetInfo>,
) {
    if kind == FILE {
        world.resource_mut::<Closet>().parts = found
            .into_iter()
            .map(|info| (info.name, info.id))
            .collect();
        return;
    }
    let name = if kind == KEPT { KEPT_NAME } else { WORN_NAME };
    let Some(info) = found.into_iter().find(|info| info.name == name)
    else {
        return;
    };
    let mut closet = world.resource_mut::<Closet>();
    match kind {
        KEPT => closet.kept = Some(info.id),
        _ => closet.worn = Some(info.id),
    }
    let ticket = world.read_asset(info.id);
    starting(world, ticket, Asked::Read(kind));
}

/// Once the store is read: the files whole are those the `worn` read
/// names whose parts are all there.
fn started(world: &mut World) {
    let mut closet = world.resource_mut::<Closet>();
    let whole = closet
        .written
        .parts
        .iter()
        .filter(|(file, count)| {
            (0..**count).all(|at| {
                closet.parts.contains_key(&part_name(file, at))
            })
        })
        .map(|(file, count)| (file.clone(), *count))
        .collect();
    closet.whole = whole;
    world.resource_mut::<Wardrobe>().read = true;
}

/// Takes in that the first part come of the file downloaded was
/// written: the next is asked for, or the file is whole.
fn written(world: &mut World) {
    let mut closet = world.resource_mut::<Closet>();
    let Some(download) = &mut closet.download else {
        return;
    };
    if !download.came.is_empty() {
        download.came.remove(0);
        download.next += 1;
    }
    if !download.came.is_empty() {
        return;
    }
    if download.ended {
        let (file, parts) = (download.file.clone(), download.next);
        closet.download = None;
        closet.whole.insert(file, parts);
    } else {
        fetch(world);
    }
}

/// Asks for the next part of the file downloaded.
fn fetch(world: &mut World) {
    let Some(download) = world.resource::<Closet>().download.clone()
    else {
        return;
    };
    let offset = u64::from(download.next) * FILE_PART as u64;
    let request = world.resource::<Market>().server.asset_file_part(
        &download.id,
        offset,
        FILE_PART as u64,
    );
    let call = Call::Part(download.file, download.next);
    market::send(world, market::Call::Wardrobe(call), request);
}

/// Takes in part `at` of the file `file`.
pub(crate) fn came(
    world: &mut World,
    file: String,
    at: u32,
    reply: Result<Reply, Failure>,
) {
    let mut closet = world.resource_mut::<Closet>();
    let Some(download) = &mut closet.download else {
        return;
    };
    if download.file != file
        || download.next != at
        || download.ended
        || !download.came.is_empty()
    {
        return;
    }
    let cut = bytes(reply)
        .map_err(|why| why.words())
        .and_then(|bytes| cut(at, bytes));
    match cut {
        Ok((parts, ended)) => {
            download.ended = ended;
            download.came = parts;
            // Nothing more came: the parts before were all.
            if download.came.is_empty() {
                let (file, parts) =
                    (download.file.clone(), download.next);
                closet.download = None;
                closet.whole.insert(file, parts);
            }
        }
        Err(why) => {
            warn(&format!("the look {file} did not come: {why}"));
            closet.download = None;
            closet.missed.insert(file, now());
        }
    }
}

/// Does the next thing the store needs, when nothing is being
/// written: keeps what changed, writes a part come, tells the realm
/// what is worn, deletes a part no longer worn, or begins to download
/// a file worn.
fn tend(world: &mut World) {
    let closet = world.resource::<Closet>();
    if !closet.ready() || closet.writing() {
        return;
    }
    if closet.keep {
        return write_kept(world);
    }
    if let Some(download) = &closet.download
        && let Some(bytes) = download.came.first()
    {
        let name = part_name(&download.file, download.next);
        let bytes = bytes.clone();
        return write_part(world, name, bytes);
    }
    let wardrobe = world.resource::<Wardrobe>();
    if !wardrobe.read {
        return;
    }
    let worn = wardrobe.worn(&closet.whole);
    if worn != closet.written || closet.worn.is_none() {
        return write_worn(world, worn);
    }
    if !closet.play.is_empty() {
        played(world);
    }
    let closet = world.resource::<Closet>();
    let downloading = closet
        .download
        .as_ref()
        .map(|download| download.file.as_str());
    if let Some((name, id)) = closet.unworn(downloading) {
        let ticket = world.delete_asset(id);
        if let Some((file, _)) = split_part(&name) {
            world.resource_mut::<Closet>().whole.remove(file);
        }
        remember(world, ticket, Asked::Delete(name));
        return;
    }
    if closet.download.is_none()
        && world.resource::<Market>().signed_in()
    {
        begin(world);
    }
}

fn write_kept(world: &mut World) {
    let kept = world.resource::<Wardrobe>().kept();
    let Ok(bytes) = serde_json::to_vec(&kept) else {
        return;
    };
    let (ticket, asked) = match world.resource::<Closet>().kept {
        Some(id) => {
            (world.update_asset(id, &bytes), Asked::Write(KEPT, None))
        }
        None => (
            world.create_asset(KEPT, KEPT_NAME, &bytes),
            Asked::Make(KEPT, None),
        ),
    };
    if remember(world, ticket, asked) {
        world.resource_mut::<Closet>().keep = false;
    }
}

fn write_part(world: &mut World, name: String, bytes: Vec<u8>) {
    if let Some(reveal) = world.get_resource::<Reveal>() {
        reveal.spend(bytes.len());
    }
    let ticket = match world.resource::<Closet>().parts.get(&name) {
        Some(id) => world.update_asset(*id, &bytes),
        None => world.create_asset(FILE, &name, &bytes),
    };
    remember(world, ticket, Asked::Part(name));
}

fn write_worn(world: &mut World, worn: Worn) {
    let bytes = worn.bytes();
    let (ticket, asked) = match world.resource::<Closet>().worn {
        Some(id) => (
            world.update_asset(id, &bytes),
            Asked::Write(WORN, Some(worn)),
        ),
        None => (
            world.create_asset(WORN, WORN_NAME, &bytes),
            Asked::Make(WORN, Some(worn)),
        ),
    };
    remember(world, ticket, asked);
}

/// Plays the emotes of the agents the realm is told wear the one
/// chosen, and forgets those that wear none.
fn played(world: &mut World) {
    // Waiting changes nothing shown.
    let names = std::mem::take(
        &mut world
            .resource_mut::<Closet>()
            .bypass_change_detection()
            .play,
    );
    let mut waiting = Vec::new();
    for name in names {
        let wardrobe = world.resource::<Wardrobe>();
        let wanted = wardrobe
            .wearing
            .of(Target::Agent, &name)
            .and_then(|outfit| outfit.emote.as_deref())
            .and_then(|id| wardrobe.file(id));
        let told = world
            .resource::<Closet>()
            .written
            .agents
            .get(&name)
            .and_then(|outfit| outfit.emote.clone());
        match wanted {
            Some(file) if told.as_ref() == Some(&file) => {
                emit_play(world, &name)
            }
            Some(_) => waiting.push(name),
            None => {}
        }
    }
    world
        .resource_mut::<Closet>()
        .bypass_change_detection()
        .play = waiting;
}

/// Begins to download the first file worn and owned that is not
/// whole in the store, unless it could not be lately.
fn begin(world: &mut World) {
    let now = now();
    let wanted = world.resource::<Wardrobe>().wanted();
    let closet = world.resource::<Closet>();
    let next = wanted.into_iter().find(|(file, _)| {
        !closet.whole.contains_key(file)
            && closet
                .missed
                .get(file)
                .is_none_or(|at| now.saturating_sub(*at) >= RETRY)
    });
    let Some((file, id)) = next else {
        return;
    };
    let mut closet = world.resource_mut::<Closet>();
    closet.missed.remove(&file);
    closet.download = Some(Download {
        file,
        id,
        next: 0,
        came: Vec::new(),
        ended: false,
    });
    fetch(world);
}

/// Has the plugin called each frame while a request of the store's
/// waits to be taken, or a turntable turns.
fn animate(
    closet: Res<Closet>,
    turning: Res<Turning>,
    animate: Option<ResMut<Animate>>,
) {
    let on = closet.stalled || turning.moving();
    if let Some(mut animate) = animate
        && animate.0 != on
    {
        animate.0 = on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_comes_in_parts_and_is_joined_in_their_order() {
        let file = (0..FILE_PART * 2 + 10)
            .map(|at| (at % 251) as u8)
            .collect::<Vec<_>>();
        let mut joined = Vec::new();
        let mut at = 0;
        loop {
            let from = (at as usize * FILE_PART).min(file.len());
            let to = (from + FILE_PART).min(file.len());
            let (parts, last) =
                cut(at, file[from..to].to_vec()).unwrap();
            for part in parts {
                joined.extend(part);
            }
            at += 1;
            if last {
                break;
            }
        }
        assert_eq!(at, looks::parts(file.len()));
        assert_eq!(joined, file);

        // A file of whole parts ends with an empty answer.
        assert!(!cut(0, vec![1; FILE_PART]).unwrap().1);
        assert_eq!(cut(1, Vec::new()).unwrap(), (Vec::new(), true));
        // An empty file is one empty part.
        assert_eq!(
            cut(0, Vec::new()).unwrap(),
            (vec![Vec::new()], true)
        );
        // The whole file at once is cut here.
        let (parts, last) = cut(0, vec![7; FILE_PART + 3]).unwrap();
        assert!(last);
        assert_eq!(
            parts.iter().map(Vec::len).collect::<Vec<_>>(),
            [FILE_PART, 3]
        );
        assert!(cut(1, vec![7; FILE_PART + 3]).is_err());
        assert_eq!(
            split_part("agent-skin-sakura@1.0.0#2"),
            Some(("agent-skin-sakura@1.0.0", 2))
        );
        assert_eq!(split_part("odd"), None);
    }

    #[test]
    fn parts_no_worn_names_are_found_but_those_downloading() {
        let mut closet = Closet::default();
        let file = Worn::file("agent-skin-sakura", "1.0.0");
        let old = Worn::file("agent-skin-sakura", "0.9.0");
        for (at, name) in [
            part_name(&file, 0),
            part_name(&file, 1),
            part_name(&old, 0),
        ]
        .into_iter()
        .enumerate()
        {
            closet.parts.insert(name, AssetId(at as u64));
        }
        closet.written.parts.insert(file.clone(), 2);
        assert_eq!(
            closet.unworn(None),
            Some((part_name(&old, 0), AssetId(2)))
        );
        assert_eq!(closet.unworn(Some(&old)), None);
        closet.written.parts.insert(file.clone(), 1);
        assert_eq!(
            closet.unworn(Some(&old)),
            Some((part_name(&file, 1), AssetId(1)))
        );
    }
}
