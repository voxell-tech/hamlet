//! The reading of what is worn from the engine's asset store: the
//! bazaar's one `looks.worn` asset, held so that each change is told,
//! and the `looks.file` parts of each file it names, each file read
//! once and joined. A few requests are made a call, and only what
//! the realm has not been given is read.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use hamlet_engine_sdk::{AssetId, AssetInfo, Failure, Reply, Ticket};
use looks::{FILE, WORN, WORN_NAME, Worn, part_name};

/// How the closet reads.
#[derive(
    Clone, Copy, PartialEq, Debug, Default, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct ClosetSettings {
    /// Seconds between lists of the store while it holds no `worn`,
    /// or its parts are not all there.
    pub relist: f32,
    /// The most requests made in one call, and waiting at once.
    pub asks: usize,
    pub in_flight: usize,
}

/// A request to the store.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Ask {
    ListWorn,
    ListFiles,
    Hold(AssetId),
    ReadWorn(AssetId),
    /// Part `at` of `file`.
    ReadPart {
        file: String,
        at: u32,
        id: AssetId,
    },
}

impl Ask {
    /// What it asks, for the log.
    fn what(&self) -> String {
        match self {
            Ask::ListWorn => format!("listing {WORN}"),
            Ask::ListFiles => format!("listing {FILE}"),
            Ask::Hold(_) => format!("holding {WORN_NAME}"),
            Ask::ReadWorn(_) => format!("reading {WORN_NAME}"),
            Ask::ReadPart { file, at, .. } => {
                format!("reading part {at} of {file}")
            }
        }
    }
}

/// What is worn, and the bytes of each file it names that the realm
/// was not given before.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Delivery {
    pub worn: Worn,
    pub files: Vec<(String, Vec<u8>)>,
}

/// The reading of what is worn.
#[derive(Default)]
pub struct Closet {
    /// The `worn` asset, once listed, and whether it is held.
    worn_id: Option<AssetId>,
    held: bool,
    /// True when `worn` is to be read again.
    stale: bool,
    /// What is worn as last read, being completed.
    worn: Option<Worn>,
    /// The id of each part listed, by its name.
    part_ids: HashMap<String, AssetId>,
    /// Clock time of the last list of each type.
    listed: BTreeMap<&'static str, f32>,
    /// The parts read of each file being read.
    reading: BTreeMap<String, Vec<Option<Vec<u8>>>>,
    /// Files whole and given, or to give, and those that could not
    /// be read.
    known: BTreeSet<String>,
    failed: BTreeSet<String>,
    /// Files whole and not yet given.
    whole: Vec<(String, Vec<u8>)>,
    /// What waits for its answer.
    asked: HashMap<Ticket, Ask>,
    /// True when what is worn was read whole and not yet given.
    ready: bool,
    /// What was last given: nothing is worn until a `worn` is read.
    given: Option<Worn>,
    /// Why requests failed, each told once, and those not yet
    /// taken to the log.
    told: BTreeSet<String>,
    notes: Vec<String>,
}

impl Closet {
    /// The requests to make at clock time `now`, within `set`.
    pub fn asks(
        &mut self,
        set: &ClosetSettings,
        now: f32,
    ) -> Vec<Ask> {
        let mut out = Vec::new();
        let room = set
            .asks
            .min(set.in_flight.saturating_sub(self.asked.len()));
        let waiting = |closet: &Self, ask: &Ask| {
            closet.asked.values().any(|asked| asked == ask)
        };
        let due = |closet: &Self, kind: &'static str| {
            closet
                .listed
                .get(kind)
                .is_none_or(|at| now - at >= set.relist)
        };
        let mut wants = Vec::new();
        match self.worn_id {
            None if due(self, WORN) => wants.push(Ask::ListWorn),
            None => {}
            Some(id) => {
                if !self.held {
                    wants.push(Ask::Hold(id));
                }
                if self.stale {
                    wants.push(Ask::ReadWorn(id));
                }
            }
        }
        let mut unlisted = false;
        for (file, parts) in &self.reading {
            for (at, part) in parts.iter().enumerate() {
                if part.is_some() {
                    continue;
                }
                match self.part_ids.get(&part_name(file, at as u32)) {
                    Some(&id) => wants.push(Ask::ReadPart {
                        file: file.clone(),
                        at: at as u32,
                        id,
                    }),
                    None => unlisted = true,
                }
            }
        }
        if unlisted && due(self, FILE) {
            wants.insert(0, Ask::ListFiles);
        }
        for ask in wants {
            if out.len() >= room {
                break;
            }
            if !waiting(self, &ask) {
                match ask {
                    Ask::ListWorn => {
                        self.listed.insert(WORN, now);
                    }
                    Ask::ListFiles => {
                        self.listed.insert(FILE, now);
                    }
                    Ask::ReadWorn(_) => self.stale = false,
                    _ => {}
                }
                out.push(ask);
            }
        }
        out
    }

    /// `ask` was made, and its answer comes with `ticket`; 0 for one
    /// the engine did not take.
    pub fn sent(&mut self, ticket: Ticket, ask: Ask) {
        if ticket == 0 {
            self.missed(&ask);
            return;
        }
        self.asked.insert(ticket, ask);
    }

    /// True when the answer of `ticket` is the closet's.
    pub fn owns(&self, ticket: Ticket) -> bool {
        self.asked.contains_key(&ticket)
    }

    /// Takes the answer of `ticket`. False when it is not the
    /// closet's.
    pub fn answer(
        &mut self,
        ticket: Ticket,
        reply: Result<Reply, Failure>,
    ) -> bool {
        let Some(ask) = self.asked.remove(&ticket) else {
            return false;
        };
        match (ask, reply) {
            (Ask::ListWorn, Ok(Reply::Listed(all))) => {
                self.listed_worn(&all);
            }
            (Ask::ListFiles, Ok(Reply::Listed(all))) => {
                self.listed_files(all);
            }
            (Ask::Hold(id), Ok(_)) => {
                self.held = Some(id) == self.worn_id
            }
            // Not held, it is not asked again: its changes are not
            // told, and the closet keeps what it read.
            (Ask::Hold(_), Err(why)) => {
                self.held = true;
                self.tell(format!("holding {WORN_NAME}: {why}"));
            }
            (Ask::ReadWorn(id), Ok(Reply::Read(asset)))
                if Some(id) == self.worn_id =>
            {
                match Worn::read(&asset.bytes) {
                    Ok(worn) => self.read_worn(worn),
                    Err(why) => {
                        self.tell(format!("{WORN_NAME}: {why}"))
                    }
                }
            }
            (
                Ask::ReadPart { file, at, .. },
                Ok(Reply::Read(asset)),
            ) => {
                self.read_part(&file, at, asset.bytes);
            }
            (ask, Err(why)) => {
                self.tell(format!("{}: {why}", ask.what()));
                self.missed(&ask);
            }
            _ => {}
        }
        self.settle();
        true
    }

    /// The asset `asset` is at a new version.
    pub fn changed(&mut self, asset: AssetId) {
        if Some(asset) == self.worn_id {
            self.stale = true;
        }
    }

    /// The asset `asset` is gone. With `worn` gone, nothing is worn.
    pub fn gone(&mut self, asset: AssetId) {
        if Some(asset) == self.worn_id {
            self.worn_id = None;
            self.held = false;
            self.listed.remove(WORN);
            self.read_worn(Worn::default());
            self.settle();
        }
    }

    /// What is worn and the files not given before, once all of it
    /// is read.
    pub fn take(&mut self) -> Option<Delivery> {
        if !self.ready {
            return None;
        }
        self.ready = false;
        let worn = self.worn.clone()?;
        self.given = Some(worn.clone());
        Some(Delivery {
            worn,
            files: std::mem::take(&mut self.whole),
        })
    }

    fn listed_worn(&mut self, all: &[AssetInfo]) {
        match all.iter().find(|info| info.name == WORN_NAME) {
            Some(info) => {
                self.worn_id = Some(info.id);
                self.held = false;
                self.stale = true;
            }
            // Nothing is worn until the bazaar says what is.
            None if self.given.is_some() => {
                self.read_worn(Worn::default());
            }
            None => {}
        }
    }

    /// Keeps the id of each part, and gives up on a file whose parts
    /// are not all there.
    fn listed_files(&mut self, all: Vec<AssetInfo>) {
        self.part_ids = all
            .into_iter()
            .map(|info| (info.name, info.id))
            .collect();
        let lacking = self
            .reading
            .iter()
            .filter(|(file, parts)| {
                (0..parts.len() as u32).any(|at| {
                    !self.part_ids.contains_key(&part_name(file, at))
                })
            })
            .map(|(file, _)| file.clone())
            .collect::<Vec<_>>();
        for file in lacking {
            self.reading.remove(&file);
            self.failed.insert(file.clone());
            self.tell(format!("{file} is not in the store whole"));
        }
    }

    /// Begins to read what `worn` names, of the files not known, and
    /// forgets those it no longer names.
    fn read_worn(&mut self, worn: Worn) {
        let files =
            worn.files().map(str::to_owned).collect::<BTreeSet<_>>();
        self.known.retain(|file| files.contains(file));
        // What could not be read is tried again.
        self.failed.clear();
        self.reading.retain(|file, _| files.contains(file));
        self.whole.retain(|(file, _)| files.contains(file));
        for file in &files {
            if self.known.contains(file)
                || self.failed.contains(file)
                || self.reading.contains_key(file)
            {
                continue;
            }
            let parts =
                worn.parts.get(file).copied().unwrap_or(1).max(1);
            self.reading
                .insert(file.clone(), vec![None; parts as usize]);
        }
        self.worn = Some(worn);
        // Parts written since the last list are listed again.
        self.listed.remove(FILE);
    }

    fn read_part(&mut self, file: &str, at: u32, bytes: Vec<u8>) {
        let Some(parts) = self.reading.get_mut(file) else {
            return;
        };
        if let Some(part) = parts.get_mut(at as usize) {
            *part = Some(bytes);
        }
        if parts.iter().all(Option::is_some) {
            let parts = self.reading.remove(file).unwrap_or_default();
            let joined =
                parts.into_iter().flatten().flatten().collect();
            self.known.insert(file.to_owned());
            self.whole.push((file.to_owned(), joined));
        }
    }

    /// A request that came to nothing.
    fn missed(&mut self, ask: &Ask) {
        match ask {
            Ask::ReadWorn(_) => {
                // Perhaps taken away: it is listed again.
                self.worn_id = None;
                self.held = false;
            }
            Ask::ReadPart { file, .. } => {
                self.reading.remove(file);
                self.failed.insert(file.clone());
            }
            Ask::Hold(_) | Ask::ListWorn | Ask::ListFiles => {}
        }
    }

    /// Marks what is worn ready once every file it names is read or
    /// could not be.
    fn settle(&mut self) {
        let Some(worn) = &self.worn else {
            return;
        };
        if !self.reading.is_empty() {
            return;
        }
        let new = self.given.as_ref() != Some(worn)
            || !self.whole.is_empty();
        if new {
            self.ready = true;
        }
    }

    /// Notes why for the log, once.
    fn tell(&mut self, why: String) {
        if self.told.insert(why.clone()) {
            self.notes.push(why);
        }
    }

    /// What went wrong since last asked, for the log.
    pub fn notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }
}

#[cfg(test)]
mod tests;
