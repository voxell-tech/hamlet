//! The plugin's own database, kept by the host: named tables of
//! records, each an id, bytes the host does not read and a few fields
//! to find it by. It needs the capability `db`; without it a call is
//! an error. A table is 1 to a few dozen bytes of `a-z`, `0-9`, `_`
//! and `-`, and is there once it holds a record. The host caps the
//! tables, the records, the bytes and the fields, and the records of
//! a page. Records are in the order of their ids. Another plugin
//! cannot reach them: share records through a collection.

use std::marker::PhantomData;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::bindings::hamlet::plugin::host;
pub use crate::bindings::hamlet::plugin::host::{
    DbField, DbPage, DbRecord,
};

fn wired(fields: &[(&str, &str)]) -> Vec<DbField> {
    fields
        .iter()
        .map(|(name, value)| DbField {
            name: name.to_string(),
            value: value.to_string(),
        })
        .collect()
}

/// Sets the record `id` of `table`, replacing it.
pub fn put(
    table: &str,
    id: &str,
    bytes: &[u8],
    fields: &[(&str, &str)],
) -> Result<(), String> {
    host::db_put(table, id, bytes, &wired(fields))
}

/// The record `id` of `table`, if it is there.
pub fn get(
    table: &str,
    id: &str,
) -> Result<Option<DbRecord>, String> {
    host::db_get(table, id)
}

/// Removes the record `id` of `table`, if it is there.
pub fn delete(table: &str, id: &str) -> Result<(), String> {
    host::db_delete(table, id)
}

/// A page of `table`, from the first id after `after`, at most
/// `limit` records. The host caps a page too: where
/// [`DbPage::next`] is an id, ask again `after` it.
pub fn list(
    table: &str,
    after: Option<&str>,
    limit: u32,
) -> Result<DbPage, String> {
    host::db_list(table, after, limit)
}

/// A page of the records of `table` whose field `field` is `value`,
/// as [`list`] pages.
pub fn find(
    table: &str,
    field: &str,
    value: &str,
    after: Option<&str>,
    limit: u32,
) -> Result<DbPage, String> {
    host::db_find(table, field, value, after, limit)
}

/// A record of a [`Table`]: its id, its value and its fields.
pub struct Row<T> {
    pub id: String,
    pub value: T,
    pub fields: Vec<(String, String)>,
}

/// A page of the rows of a [`Table`].
pub struct Rows<T> {
    pub rows: Vec<Row<T>>,
    /// The id to give as `after` for the next page, when there is
    /// one.
    pub next: Option<String>,
}

/// A table whose records hold a `T`, written with `postcard`. It
/// holds only a name: `const ITEMS: Table<Item> =
/// Table::new("items")`.
pub struct Table<T> {
    name: &'static str,
    kind: PhantomData<fn() -> T>,
}

impl<T: Serialize + DeserializeOwned> Table<T> {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            kind: PhantomData,
        }
    }

    /// Sets the record `id`, with `fields` to find it by.
    pub fn put(
        &self,
        id: &str,
        value: &T,
        fields: &[(&str, &str)],
    ) -> Result<(), String> {
        let bytes = postcard::to_allocvec(value)
            .map_err(|e| e.to_string())?;
        put(self.name, id, &bytes, fields)
    }

    /// The value of the record `id`, if it is there.
    pub fn get(&self, id: &str) -> Result<Option<T>, String> {
        get(self.name, id)?
            .map(|record| decode(&record.bytes))
            .transpose()
    }

    /// Removes the record `id`, if it is there.
    pub fn delete(&self, id: &str) -> Result<(), String> {
        delete(self.name, id)
    }

    /// A page of the rows, as [`list`] pages.
    pub fn list(
        &self,
        after: Option<&str>,
        limit: u32,
    ) -> Result<Rows<T>, String> {
        rows(list(self.name, after, limit)?)
    }

    /// A page of the rows whose field `field` is `value`, as [`find`]
    /// pages.
    pub fn find(
        &self,
        field: &str,
        value: &str,
        after: Option<&str>,
        limit: u32,
    ) -> Result<Rows<T>, String> {
        rows(find(self.name, field, value, after, limit)?)
    }
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    postcard::from_bytes(bytes).map_err(|e| e.to_string())
}

fn rows<T: DeserializeOwned>(
    page: DbPage,
) -> Result<Rows<T>, String> {
    let rows = page
        .records
        .into_iter()
        .map(|record| {
            Ok(Row {
                value: decode(&record.bytes)?,
                id: record.id,
                fields: record
                    .fields
                    .into_iter()
                    .map(|field| (field.name, field.value))
                    .collect(),
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(Rows {
        rows,
        next: page.next,
    })
}
