//! The ids rows are kept under, and who acted.

use std::fmt;
use std::str::FromStr;

use diesel::deserialize::{FromSql, FromSqlRow};
use diesel::expression::AsExpression;
use diesel::serialize::{IsNull, Output, ToSql};
use diesel::sql_types::{BigInt, Text};
use diesel::sqlite::{Sqlite, SqliteValue};
use serde::{Deserialize, Serialize};

macro_rules! integer_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
            Serialize, Deserialize, AsExpression, FromSqlRow,
        )]
        #[diesel(sql_type = BigInt)]
        #[serde(transparent)]
        pub struct $name(pub i64);

        impl ToSql<BigInt, Sqlite> for $name {
            fn to_sql<'b>(
                &'b self,
                out: &mut Output<'b, '_, Sqlite>,
            ) -> diesel::serialize::Result {
                out.set_value(self.0);
                Ok(IsNull::No)
            }
        }

        impl FromSql<BigInt, Sqlite> for $name {
            fn from_sql(
                value: SqliteValue<'_, '_, '_>,
            ) -> diesel::deserialize::Result<Self> {
                Ok(Self(<i64 as FromSql<BigInt, Sqlite>>::from_sql(
                    value,
                )?))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

integer_id!(
    /// A row of `chief`.
    ChiefId
);
integer_id!(
    /// A row of `scholar`.
    ScholarId
);
integer_id!(
    /// A row of `agent`: the Hand, a hire or a worker.
    AgentId
);
integer_id!(
    /// A row of `conversation`.
    ConversationId
);
integer_id!(
    /// A row of `task`.
    TaskId
);
integer_id!(
    /// A row of `wait`.
    WaitId
);
integer_id!(
    /// A row of `road`.
    RoadId
);
integer_id!(
    /// A row of `job`.
    JobId
);
integer_id!(
    /// A row of `hire_merge`.
    MergeId
);
integer_id!(
    /// A row of `dream`.
    DreamId
);
integer_id!(
    /// A row of `skill_offer`.
    OfferId
);

/// A row of `hamlet`: lowercase letters, digits and hyphens, never
/// given twice (H36).
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    AsExpression,
    FromSqlRow,
)]
#[diesel(sql_type = Text)]
#[serde(transparent)]
pub struct HamletId(pub String);

impl HamletId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ToSql<Text, Sqlite> for HamletId {
    fn to_sql<'b>(
        &'b self,
        out: &mut Output<'b, '_, Sqlite>,
    ) -> diesel::serialize::Result {
        out.set_value(self.0.as_str());
        Ok(IsNull::No)
    }
}

impl FromSql<Text, Sqlite> for HamletId {
    fn from_sql(
        value: SqliteValue<'_, '_, '_>,
    ) -> diesel::deserialize::Result<Self> {
        Ok(Self(<String as FromSql<Text, Sqlite>>::from_sql(value)?))
    }
}

impl fmt::Display for HamletId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Who did something: a chief, an agent, or the realm itself.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    AsExpression,
    FromSqlRow,
)]
#[diesel(sql_type = Text)]
#[serde(into = "String", try_from = "String")]
pub enum Actor {
    Chief(ChiefId),
    Agent(AgentId),
    Realm,
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Chief(chief) => write!(f, "chief:{chief}"),
            Self::Agent(agent) => write!(f, "agent:{agent}"),
            Self::Realm => f.write_str("realm"),
        }
    }
}

impl FromStr for Actor {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let id = |rest: &str| {
            rest.parse::<i64>()
                .map_err(|_| format!("not an actor: {text:?}"))
        };
        match text.split_once(':') {
            Some(("chief", rest)) => {
                Ok(Self::Chief(ChiefId(id(rest)?)))
            }
            Some(("agent", rest)) => {
                Ok(Self::Agent(AgentId(id(rest)?)))
            }
            None if text == "realm" => Ok(Self::Realm),
            _ => Err(format!("not an actor: {text:?}")),
        }
    }
}

impl From<Actor> for String {
    fn from(actor: Actor) -> Self {
        actor.to_string()
    }
}

impl TryFrom<String> for Actor {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        text.parse()
    }
}

impl ToSql<Text, Sqlite> for Actor {
    fn to_sql<'b>(
        &'b self,
        out: &mut Output<'b, '_, Sqlite>,
    ) -> diesel::serialize::Result {
        out.set_value(self.to_string());
        Ok(IsNull::No)
    }
}

impl FromSql<Text, Sqlite> for Actor {
    fn from_sql(
        value: SqliteValue<'_, '_, '_>,
    ) -> diesel::deserialize::Result<Self> {
        Ok(<String as FromSql<Text, Sqlite>>::from_sql(value)?
            .parse()?)
    }
}
