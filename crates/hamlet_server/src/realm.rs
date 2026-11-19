//! A realm this server holds: claimed, laid out, with its database
//! open. A new realm is made with its first chief and its Hand.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use hamlet_store::Store;
use hamlet_store::ids::{Actor, ChiefId};

use crate::change::Change;
use crate::claim::Claim;
use crate::layout::Layout;
use crate::society::{chiefs, hand};

/// The Hand's character in a new realm.
const HAND: &str = include_str!("../assets/hand.md");

pub(crate) struct Realm {
    layout: Layout,
    // Declared before the claim, so a dropped realm closes its
    // database before it lets the folder go.
    store: Store,
    chief: ChiefId,
    _claim: Claim,
}

impl Realm {
    /// Claims the realm in `dir` for the chief named `chief`, making
    /// a new realm when the folder holds none. A realm missing a
    /// part, or with another realm's part, is refused (S25).
    pub fn open(dir: &Path, chief: &str, at: i64) -> Result<Self> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("making {}", dir.display()))?;
        let root = dir.canonicalize().with_context(|| {
            format!("resolving {}", dir.display())
        })?;
        let claim = Claim::take(&root)?;
        let layout = Layout::new(&root);
        let mut store = if layout.database().try_exists()? {
            let mut store = Store::open(&layout.database())?;
            let kept = std::fs::read_to_string(layout.identity())
                .context(
                    "reading realm.id; restore the whole realm from \
                     a backup if it is missing",
                )?;
            ensure!(
                kept == store.realm()?,
                "the realm's database belongs to another realm; \
                 restore the matching database and realm.id from a \
                 whole backup"
            );
            store
        } else {
            ensure!(
                !layout.identity().try_exists()?,
                "the realm's database is missing; restore the whole \
                 realm from a backup"
            );
            make(&layout, at)?
        };
        let chief = chiefs::ensure(store.connection(), chief, at)?;
        Ok(Self {
            layout,
            store,
            chief,
            _claim: claim,
        })
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn store(&mut self) -> &mut Store {
        &mut self.store
    }

    /// The chief this server runs as (C7).
    pub fn chief(&self) -> ChiefId {
        self.chief
    }
}

/// Makes a new realm's database with its Hand, then its identity
/// file.
fn make(layout: &Layout, at: i64) -> Result<Store> {
    let identity = uuid::Uuid::new_v4().to_string();
    let mut store = Store::create(&layout.database(), &identity, at)?;
    let mut tx = store.immediate()?;
    let mut cx = Change::new(&mut tx, at, Actor::Realm);
    hand::make(&mut cx, HAND)?;
    tx.commit()?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(layout.identity())
        .context("making realm.id")?;
    file.write_all(identity.as_bytes())?;
    file.sync_all()?;
    #[cfg(unix)]
    std::fs::File::open(layout.root())?.sync_all()?;
    Ok(store)
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::society::agents;

    #[test]
    fn a_realm_is_made_once_with_its_hand_and_refused_when_broken() {
        let dir = Scratch::new("server-realm-open");
        let at = 1;
        let mut realm = Realm::open(&dir, "chief", at).unwrap();
        let db = realm.store().connection();
        let hand = hand::id(db).unwrap();
        assert!(agents::get(db, hand).unwrap().is_living());
        drop(realm);
        assert!(
            Realm::open(&dir, "chief", at).is_ok(),
            "it opens again"
        );
        std::fs::write(dir.join("realm.id"), "another").unwrap();
        assert!(Realm::open(&dir, "chief", at).is_err());
        std::fs::remove_file(dir.join("realm.sqlite")).unwrap();
        assert!(Realm::open(&dir, "chief", at).is_err());
    }
}
