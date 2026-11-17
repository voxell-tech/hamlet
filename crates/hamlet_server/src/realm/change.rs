//! A change to the realm in progress: its transaction, its time, who
//! makes it, and what it touched for the clients to see again.

use std::collections::BTreeSet;

use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId, HamletId, ScholarId};

use crate::society::refusal::Refusal;

/// What a change does: a defect as an error, an expected refusal as
/// `Err` inside.
pub(crate) type Done<T> = anyhow::Result<Result<T, Refusal>>;

/// Ends a change with `refusal`.
macro_rules! refuse {
    ($refusal:expr) => {
        return Ok(Err($refusal))
    };
}
pub(crate) use refuse;

/// Unwraps a change's result, or ends the change with its refusal.
macro_rules! done {
    ($result:expr) => {
        match $result? {
            Ok(value) => value,
            Err(refusal) => return Ok(Err(refusal)),
        }
    };
}
pub(crate) use done;

/// The entities a change touched.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Touched {
    pub agents: BTreeSet<AgentId>,
    pub scholars: BTreeSet<ScholarId>,
    pub hamlets: BTreeSet<HamletId>,
    /// Jobs were kept that the runner has not seen.
    pub jobs: bool,
}

pub(crate) struct Change<'a> {
    pub db: &'a mut Db,
    pub at: i64,
    pub actor: Actor,
    pub touched: Touched,
}

impl<'a> Change<'a> {
    pub fn new(db: &'a mut Db, at: i64, actor: Actor) -> Self {
        Self {
            db,
            at,
            actor,
            touched: Touched::default(),
        }
    }
}

/// Why a change stopped before its end.
enum Stop {
    Refused(Refusal),
    Defect(anyhow::Error),
}

impl From<diesel::result::Error> for Stop {
    fn from(error: diesel::result::Error) -> Self {
        Self::Defect(error.into())
    }
}

/// Makes the change `make` by `actor` at `at` in a savepoint of the
/// open transaction. A refusal leaves nothing of it.
pub(crate) fn run<T>(
    db: &mut Db,
    at: i64,
    actor: Actor,
    make: impl FnOnce(&mut Change<'_>) -> Done<T>,
) -> anyhow::Result<(Result<T, Refusal>, Touched)> {
    use diesel::Connection;

    let mut touched = Touched::default();
    let made = db.transaction::<_, Stop, _>(|db| {
        let mut cx = Change::new(db, at, actor);
        let made = make(&mut cx).map_err(Stop::Defect)?;
        touched = std::mem::take(&mut cx.touched);
        made.map_err(Stop::Refused)
    });
    match made {
        Ok(value) => Ok((Ok(value), touched)),
        Err(Stop::Refused(refusal)) => {
            Ok((Err(refusal), Touched::default()))
        }
        Err(Stop::Defect(error)) => Err(error),
    }
}
