//! A unit run with no host: a repository in a scratch folder and a
//! store in memory, through the same [`change`] and [`read`] a host's
//! call runs. It does not cover the host's stamp, the task a unit is
//! detached on, or the calls made after it.

use std::path::{Path, PathBuf};

use anyhow::Result;
use hamlet_actor::{BindParam, ColumnValue};
use hamlet_host::Caller;
use hamlet_messages::sender::{Sender, wrong_caller};
use hamlet_messages::{Changes, Message};
use rusqlite::types::{Value, ValueRef};

use super::git::{Folder, Git};
use super::store::{Sql, marker};
use super::{Admits, Asked, Before, Ended, Look, Told, change, read};

pub(crate) struct Hostless {
    folder: Folder,
    store: rusqlite::Connection,
    /// The time of the next call, moved on by one with each.
    at: i64,
}

impl Hostless {
    /// The repository at `path`, made by `make`, with `home` as the
    /// realm folder.
    pub fn new(
        home: &Path,
        path: PathBuf,
        schema: &str,
        make: impl FnOnce(&Git<'_>) -> Result<()>,
    ) -> Result<Self> {
        let store = rusqlite::Connection::open_in_memory()?;
        store
            .execute_batch(&format!("{};{schema}", marker::SCHEMA))?;
        let folder = Folder::start(path, home.to_owned(), 0, make)?;
        Ok(Self {
            folder,
            store,
            at: 0,
        })
    }

    pub fn store(&self) -> &rusqlite::Connection {
        &self.store
    }

    fn asked<M: Message>(
        &mut self,
        caller: Caller,
        message: M,
    ) -> Result<Asked<M>> {
        let Some(from) = M::From::of(Some(&caller)) else {
            return Err(wrong_caller(M::NAME, Some(caller)));
        };
        self.at += 1;
        Ok(Asked {
            caller,
            from,
            message,
            at: self.at,
        })
    }

    /// The answer [`super::Call::once`] gives, with the calls it
    /// queued.
    pub fn once<A, M, F>(
        &mut self,
        caller: Caller,
        message: M,
        work: F,
    ) -> Result<(M::Output, Vec<Told>)>
    where
        A: Admits<M::From>,
        M: Changes,
        F: FnOnce(Before<'_, A, M>) -> Result<Ended<M>>,
    {
        let asked = self.asked(caller, message)?;
        let mut held = self.folder.open(asked.at)?;
        self.store.execute_batch("BEGIN")?;
        let changed =
            change::<A, M, F>(&mut held, &self.store, &asked, work);
        self.store.execute_batch(if changed.is_ok() {
            "COMMIT"
        } else {
            "ROLLBACK"
        })?;
        changed.map(|changed| (changed.answer, changed.told))
    }

    /// The answer [`super::Call::read`] gives.
    pub fn read<A, M, F>(
        &mut self,
        caller: Caller,
        message: M,
        look: F,
    ) -> Result<M::Output>
    where
        A: Admits<M::From>,
        M: Message,
        F: FnOnce(&Look<'_, A, M>) -> Result<M::Output>,
    {
        let asked = self.asked(caller, message)?;
        let mut held = self.folder.open(asked.at)?;
        read::<A, M, _, F>(&mut held, &self.store, &asked, look)
    }
}

fn bound(params: Vec<BindParam>) -> Vec<Value> {
    params
        .into_iter()
        .map(|param| match param {
            BindParam::Null => Value::Null,
            BindParam::Integer(n) => Value::Integer(n),
            BindParam::Float(n) => Value::Real(n),
            BindParam::Text(text) => Value::Text(text),
            BindParam::Blob(bytes) => Value::Blob(bytes),
        })
        .collect()
}

impl Sql for rusqlite::Connection {
    fn select(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<Vec<Vec<ColumnValue>>> {
        let mut statement = self.prepare(sql)?;
        let width = statement.column_count();
        let mut rows = statement
            .query(rusqlite::params_from_iter(bound(params)))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let mut values = Vec::with_capacity(width);
            for at in 0..width {
                values.push(match row.get_ref(at)? {
                    ValueRef::Null => ColumnValue::Null,
                    ValueRef::Integer(n) => ColumnValue::Integer(n),
                    ValueRef::Real(n) => ColumnValue::Float(n),
                    ValueRef::Text(text) => ColumnValue::Text(
                        String::from_utf8(text.to_vec())?,
                    ),
                    ValueRef::Blob(bytes) => {
                        ColumnValue::Blob(bytes.to_vec())
                    }
                });
            }
            out.push(values);
        }
        Ok(out)
    }

    fn execute(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<u64> {
        let changed = rusqlite::Connection::execute(
            self,
            sql,
            rusqlite::params_from_iter(bound(params)),
        )?;
        Ok(u64::try_from(changed)?)
    }
}
