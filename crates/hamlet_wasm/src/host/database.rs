//! The records a plugin keeps in its own database, each call needing
//! the grant of `db`.

use hamlet_sdk::capability::Capability;

use super::State;
use crate::db::{Db, Page, Record};
use crate::interface::hamlet::plugin::host::{
    DbField, DbPage, DbRecord,
};

impl State {
    pub(super) fn db_set(
        &mut self,
        table: &str,
        id: String,
        bytes: Vec<u8>,
        fields: Vec<DbField>,
    ) -> Result<(), String> {
        self.require(Capability::Db)?;
        let fields = fields
            .into_iter()
            .map(|field| (field.name, field.value))
            .collect();
        self.database()?.put(table, Record { id, bytes, fields })
    }

    pub(super) fn db_record(
        &mut self,
        table: &str,
        id: &str,
    ) -> Result<Option<DbRecord>, String> {
        self.require(Capability::Db)?;
        Ok(self.database()?.get(table, id)?.map(record))
    }

    pub(super) fn db_remove(
        &mut self,
        table: &str,
        id: &str,
    ) -> Result<(), String> {
        self.require(Capability::Db)?;
        self.database()?.delete(table, id)
    }

    pub(super) fn db_page(
        &mut self,
        table: &str,
        after: Option<&str>,
        limit: u32,
    ) -> Result<DbPage, String> {
        self.require(Capability::Db)?;
        self.database()?.list(table, after, limit).map(page)
    }

    pub(super) fn db_search(
        &mut self,
        table: &str,
        field: &str,
        value: &str,
        after: Option<&str>,
        limit: u32,
    ) -> Result<DbPage, String> {
        self.require(Capability::Db)?;
        self.database()?
            .find(table, field, value, after, limit)
            .map(page)
    }

    fn database(&self) -> Result<&Db, String> {
        self.db.as_ref().ok_or("there is no database".into())
    }
}

fn record(record: Record) -> DbRecord {
    DbRecord {
        id: record.id,
        bytes: record.bytes,
        fields: record
            .fields
            .into_iter()
            .map(|(name, value)| DbField { name, value })
            .collect(),
    }
}

fn page(page: Page) -> DbPage {
    DbPage {
        records: page.records.into_iter().map(record).collect(),
        next: page.next,
    }
}
