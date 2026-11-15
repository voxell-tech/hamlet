//! Helpers shared by the unit tests.

use hamlet_store::ids::{Actor, AgentId, ChiefId};
use hamlet_store::{Db, Store};
use hamlet_testing::Scratch;

use crate::change::{Change, Done, run};
use crate::layout::Layout;
use crate::society::hamlets::{self, Founding};
use crate::society::hires::{Hired, Hiring};
use crate::society::refusal::Refusal;
use crate::society::{chiefs, hand};

/// A PNG of `width` by `height` pixels.
pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
    use image::{ExtendedColorType, ImageEncoder};

    let mut out = Vec::new();
    let pixels = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x * 7 % 256) as u8, (y * 13 % 256) as u8, 90])
    });
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(
            pixels.as_raw(),
            width,
            height,
            ExtendedColorType::Rgb8,
        )
        .unwrap();
    out
}

/// A new realm with its first chief and its Hand.
pub(crate) struct Realm {
    dir: Scratch,
    pub store: Store,
    pub chief: ChiefId,
    pub hand: AgentId,
    pub at: i64,
}

impl Realm {
    pub fn new() -> Self {
        let dir = Scratch::new("server-realm");
        let mut store =
            Store::create(&dir.join("realm.sqlite"), "realm-test", 1)
                .unwrap();
        let db = store.connection();
        let chief = chiefs::ensure(db, "chief", 1).unwrap();
        let mut cx = Change::new(db, 1, Actor::Realm);
        let hand = hand::make(&mut cx, "The Hand.").unwrap();
        Self {
            dir,
            store,
            chief,
            hand,
            at: 1,
        }
    }

    pub fn db(&mut self) -> &mut Db {
        self.store.connection()
    }

    /// The realm's folder, which its database is in.
    pub fn layout(&self) -> Layout {
        Layout::new(&self.dir)
    }

    /// Runs the jobs kept so far.
    pub fn work(&mut self) {
        let layout = self.layout();
        let at = self.at;
        let writing =
            crate::knowledge::runner::RepositoryWriteLock::default();
        crate::knowledge::run::due(self.db(), &layout, &writing, at)
            .unwrap();
        let left =
            hamlet_store::job::due(self.db(), i64::MAX).unwrap();
        assert!(left.is_empty(), "jobs failed: {left:?}");
    }

    /// Makes `make` by `actor` a moment after the last change, as one
    /// committed transaction.
    pub fn by<T>(
        &mut self,
        actor: Actor,
        make: impl FnOnce(&mut Change<'_>) -> Done<T>,
    ) -> Result<T, Refusal> {
        self.at += 1000;
        let at = self.at;
        let mut tx = self.store.immediate().unwrap();
        let (made, _) = run(&mut tx, at, actor, make).unwrap();
        tx.commit().unwrap();
        made
    }

    /// Makes `make` as the chief.
    pub fn chief<T>(
        &mut self,
        make: impl FnOnce(&mut Change<'_>) -> Done<T>,
    ) -> Result<T, Refusal> {
        let chief = Actor::Chief(self.chief);
        self.by(chief, make)
    }

    /// Founds a hamlet named `name` under a new mayor named `mayor`;
    /// the mayor's `hamlet` is its id.
    pub fn found(&mut self, name: &str, mayor: &str) -> Hired {
        let chief = self.chief;
        let (name, mayor) = (name.to_owned(), mayor.to_owned());
        self.chief(|cx| {
            hamlets::found(
                cx,
                Founding {
                    name,
                    chief,
                    mayor: Hiring::Blank {
                        name: mayor,
                        character: "Careful.".into(),
                    },
                    purpose: "Build it".into(),
                },
            )
        })
        .unwrap()
    }
}
