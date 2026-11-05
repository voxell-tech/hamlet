//! Markers: the outcome of a request kept under its identity, so the
//! same request asked again gets the same answer and a different one
//! under a used identity is refused (ADR 0012, S19).

use anyhow::{Result, ensure};
use diesel::prelude::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::Db;
use crate::schema::marker;

/// Where a request comes from and the key it is kept under there: a
/// runtime and its call's key, or a chief's client and its request
/// id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity<'a> {
    pub source: &'a str,
    pub key: &'a str,
}

/// A request as its marker keeps it: its kind and the SHA-256 of the
/// kind and the request in canonical JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    kind: &'static str,
    digest: Vec<u8>,
}

impl Request {
    pub fn new(
        kind: &'static str,
        request: &impl Serialize,
    ) -> Result<Self> {
        let mut text = String::new();
        canonical(&serde_json::to_value(request)?, &mut text);
        let mut hash = Sha256::new();
        hash.update(kind);
        hash.update([0]);
        hash.update(text);
        Ok(Self {
            kind,
            digest: hash.finalize().to_vec(),
        })
    }

    pub fn kind(&self) -> &'static str {
        self.kind
    }

    pub fn digest(&self) -> &[u8] {
        &self.digest
    }
}

/// What the marker of an identity keeps for a request.
#[derive(Debug, PartialEq)]
pub enum Marked<T> {
    /// Nothing is kept under the identity.
    Unmarked,
    /// The request began and its outcome is not kept yet.
    Begun,
    Outcome(T),
    /// The identity is kept for another request.
    Conflict,
}

/// The marker of `id`, read as an outcome of `request`.
pub fn read<T: DeserializeOwned>(
    db: &mut Db,
    id: Identity<'_>,
    request: &Request,
) -> Result<Marked<T>> {
    let kept = marker::table
        .filter(marker::source.eq(id.source))
        .filter(marker::key.eq(id.key))
        .select((marker::kind, marker::digest, marker::outcome))
        .first::<(String, Vec<u8>, String)>(db)
        .optional()?;
    let Some((kind, digest, outcome)) = kept else {
        return Ok(Marked::Unmarked);
    };
    if kind != request.kind || digest != request.digest {
        return Ok(Marked::Conflict);
    }
    if outcome.is_empty() {
        return Ok(Marked::Begun);
    }
    Ok(Marked::Outcome(serde_json::from_str(&outcome)?))
}

/// Marks `request` begun under `id`, which must be unmarked.
pub fn begin(
    db: &mut Db,
    id: Identity<'_>,
    request: &Request,
    at: i64,
) -> Result<()> {
    diesel::insert_into(marker::table)
        .values((
            marker::source.eq(id.source),
            marker::key.eq(id.key),
            marker::kind.eq(request.kind),
            marker::digest.eq(&request.digest),
            marker::outcome.eq(""),
            marker::at.eq(at),
        ))
        .execute(db)?;
    Ok(())
}

/// Keeps `outcome` as the answer to `request` under `id`, which is
/// unmarked or begun for the same request.
pub fn settle<T: Serialize>(
    db: &mut Db,
    id: Identity<'_>,
    request: &Request,
    outcome: &T,
    at: i64,
) -> Result<()> {
    let text = serde_json::to_string(outcome)?;
    ensure!(!text.is_empty(), "an outcome cannot be empty");
    let changed = diesel::update(
        marker::table
            .filter(marker::source.eq(id.source))
            .filter(marker::key.eq(id.key))
            .filter(marker::kind.eq(request.kind))
            .filter(marker::digest.eq(&request.digest)),
    )
    .set((marker::outcome.eq(&text), marker::at.eq(at)))
    .execute(db)?;
    if changed == 0 {
        diesel::insert_into(marker::table)
            .values((
                marker::source.eq(id.source),
                marker::key.eq(id.key),
                marker::kind.eq(request.kind),
                marker::digest.eq(&request.digest),
                marker::outcome.eq(&text),
                marker::at.eq(at),
            ))
            .execute(db)?;
    }
    Ok(())
}

/// Writes `value` as JSON with object keys in order and no spaces.
pub fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            out.push('{');
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                canonical(&map[key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}
