//! Helpers shared by the unit tests.

use hamlet_store::ids::{Actor, AgentId, ChiefId, HamletId};
use hamlet_store::{Db, Store};
use hamlet_testing::Scratch;

use crate::change::{Change, Done, Touched, run};
use crate::society::hamlets::{self, Founding};
use crate::society::hires::{Hired, Hiring};
use crate::society::refusal::Refusal;
use crate::society::{chiefs, hand};

/// A new realm with its first chief and its Hand.
pub(crate) struct Realm {
    _dir: Scratch,
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
        let chief = chiefs::ensure(db, "nixon", 1).unwrap();
        let mut cx = Change::new(db, 1, Actor::Realm);
        let hand = hand::make(&mut cx, "The Hand.").unwrap();
        Self {
            _dir: dir,
            store,
            chief,
            hand,
            at: 1,
        }
    }

    pub fn db(&mut self) -> &mut Db {
        self.store.connection()
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
        let (made, _): (_, Touched) =
            run(&mut tx, at, actor, make).unwrap();
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

    /// Founds `id` under a new mayor named `mayor`.
    pub fn found(&mut self, id: &str, mayor: &str) -> Hired {
        let chief = self.chief;
        let (id, mayor) = (HamletId(id.into()), mayor.to_owned());
        self.chief(|cx| {
            hamlets::found(
                cx,
                Founding {
                    id,
                    chief,
                    mayor: Hiring::New {
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
