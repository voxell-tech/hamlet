//! The closet reads each file once, joined from its parts, and gives
//! the realm only what it has not had.

use hamlet_engine_sdk::Asset;
use hamlet_engine_sdk::api::asset::{AssetType, TypeName};
use hamlet_engine_sdk::api::id::PluginId;
use looks::Outfit;

use super::*;

const SET: ClosetSettings = ClosetSettings {
    relist: 10.0,
    asks: 2,
    in_flight: 4,
};

/// The store as the closet sees it: assets by id.
#[derive(Default)]
struct Store {
    assets: Vec<(AssetInfo, Vec<u8>)>,
    ticket: Ticket,
    /// Every request answered.
    asked: Vec<Ask>,
}

impl Store {
    fn put(
        &mut self,
        kind: &str,
        name: &str,
        bytes: Vec<u8>,
    ) -> AssetId {
        let id = AssetId(self.assets.len() as u64 + 1);
        let info = AssetInfo {
            id,
            kind: AssetType {
                name: TypeName::try_from(kind).unwrap(),
                version: 1,
            },
            owner: PluginId::try_from("hamlet.bazaar").unwrap(),
            name: name.into(),
            version: 1,
        };
        self.assets.push((info, bytes));
        id
    }

    fn set(&mut self, id: AssetId, bytes: Vec<u8>) {
        self.assets[(id.0 - 1) as usize].1 = bytes;
    }

    fn answer(&self, ask: &Ask) -> Result<Reply, Failure> {
        let listed = |kind: &str| {
            Reply::Listed(
                self.assets
                    .iter()
                    .filter(|(info, _)| {
                        info.kind.name.as_str() == kind
                    })
                    .map(|(info, _)| info.clone())
                    .collect(),
            )
        };
        let read = |id: &AssetId| {
            self.assets
                .iter()
                .find(|(info, _)| info.id == *id)
                .map(|(info, bytes)| {
                    Reply::Read(Asset {
                        info: info.clone(),
                        bytes: bytes.clone(),
                    })
                })
                .ok_or(Failure::NotFound)
        };
        match ask {
            Ask::ListWorn => Ok(listed(WORN)),
            Ask::ListFiles => Ok(listed(FILE)),
            Ask::Hold(_) => Ok(Reply::Held(1)),
            Ask::ReadWorn(id) | Ask::ReadPart { id, .. } => read(id),
        }
    }

    /// Runs the closet a call at a time until it has nothing to ask.
    fn run(
        &mut self,
        closet: &mut Closet,
        now: f32,
    ) -> Option<Delivery> {
        for _ in 0..50 {
            let asks = closet.asks(&SET, now);
            if asks.is_empty() {
                break;
            }
            assert!(asks.len() <= SET.asks);
            let mut sent = Vec::new();
            for ask in asks {
                self.ticket += 1;
                closet.sent(self.ticket, ask.clone());
                sent.push((self.ticket, ask));
            }
            for (ticket, ask) in sent {
                let reply = self.answer(&ask);
                self.asked.push(ask);
                assert!(closet.answer(ticket, reply));
            }
        }
        closet.take()
    }

    fn reads(&self) -> usize {
        self.asked
            .iter()
            .filter(|ask| matches!(ask, Ask::ReadPart { .. }))
            .count()
    }
}

fn wearing(skin: &str, emote: Option<&str>) -> Worn {
    let mut worn = Worn::default();
    worn.agents.insert(
        "ada".into(),
        Outfit {
            skin: Some(skin.into()),
            emote: emote.map(str::to_owned),
            ..Outfit::default()
        },
    );
    for file in worn.files().map(str::to_owned).collect::<Vec<_>>() {
        worn.parts.insert(file, 2);
    }
    worn
}

#[test]
fn nothing_is_worn_while_the_store_holds_no_worn() {
    let mut store = Store::default();
    let mut closet = Closet::default();
    assert_eq!(store.run(&mut closet, 0.0), None);
    // It looks again a while later, not every call.
    assert!(closet.asks(&SET, 1.0).is_empty());
    assert_eq!(closet.asks(&SET, SET.relist), vec![Ask::ListWorn]);
}

#[test]
fn each_file_is_read_once_whole_and_dropped_when_no_longer_worn() {
    let mut store = Store::default();
    let (skin, emote) = ("skin@1", "wave@1");
    for (file, bytes) in
        [(skin, b"skin-bytes"), (emote, b"wave-bytes")]
    {
        for at in 0..2u32 {
            let half = at as usize * 5;
            store.put(
                FILE,
                &part_name(file, at),
                bytes[half..half + 5].to_vec(),
            );
        }
    }
    let worn = wearing(skin, None);
    let worn_id = store.put(WORN, WORN_NAME, worn.bytes());
    let mut closet = Closet::default();
    let given = store.run(&mut closet, 0.0).unwrap();
    assert_eq!(given.worn, worn);
    assert_eq!(
        given.files,
        vec![(skin.into(), b"skin-bytes".to_vec())]
    );
    assert_eq!(store.reads(), 2);

    // The bazaar puts an emote on too: only the emote is read.
    let both = wearing(skin, Some(emote));
    store.set(worn_id, both.bytes());
    closet.changed(worn_id);
    let given = store.run(&mut closet, 1.0).unwrap();
    assert_eq!(given.worn, both);
    assert_eq!(
        given.files,
        vec![(emote.into(), b"wave-bytes".to_vec())]
    );
    assert_eq!(store.reads(), 4);

    // Nothing changed, nothing is given.
    closet.changed(worn_id);
    assert_eq!(store.run(&mut closet, 2.0), None);

    // Taken off and put on again, the skin is read anew.
    store.set(worn_id, Worn::default().bytes());
    closet.changed(worn_id);
    assert_eq!(
        store.run(&mut closet, 3.0).unwrap().worn,
        Worn::default()
    );
    store.set(worn_id, worn.bytes());
    closet.changed(worn_id);
    let given = store.run(&mut closet, 4.0).unwrap();
    assert_eq!(given.files.len(), 1);
    assert_eq!(store.reads(), 6);

    // With the worn asset gone, nothing is worn.
    closet.gone(worn_id);
    assert_eq!(closet.take().unwrap().worn, Worn::default());
}

#[test]
fn a_file_not_in_the_store_whole_is_given_up_and_the_rest_worn() {
    let mut store = Store::default();
    store.put(FILE, &part_name("skin@1", 0), b"half".to_vec());
    let worn = wearing("skin@1", None);
    store.put(WORN, WORN_NAME, worn.bytes());
    let mut closet = Closet::default();
    let given = store.run(&mut closet, 0.0).unwrap();
    assert_eq!(given.worn, worn);
    assert!(given.files.is_empty());
    assert_eq!(closet.notes().len(), 1);
}
