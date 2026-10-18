//! A plugin's database: tables of records, one file for each table in
//! a folder of the plugin's own, under caps. A record is an id, bytes
//! the host does not read and a few fields to find it by.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::limits::Limits;

/// The folder of a plugin's folder that holds its database.
pub const DB: &str = "db";

/// The end of the name of the file of a table.
const TABLE: &str = ".tbl";
/// The start of the name of a file being written. It is not the end
/// of a table's, so a file being written is not a table.
const WRITING: &str = ".tmp-";

/// What a database may hold.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Caps {
    pub bytes: u64,
    pub tables: usize,
    pub records: usize,
    pub record_bytes: usize,
    pub fields: usize,
    pub name_bytes: usize,
    pub text_bytes: usize,
    pub page_records: usize,
    pub page_bytes: usize,
}

impl Caps {
    pub fn of(limits: &Limits) -> Self {
        Self {
            bytes: limits.db_bytes,
            tables: limits.db_tables,
            records: limits.db_records,
            record_bytes: limits.db_record_bytes,
            fields: limits.db_fields,
            name_bytes: limits.db_name_bytes,
            text_bytes: limits.db_text_bytes,
            page_records: limits.db_page_records,
            page_bytes: limits.db_page_bytes,
        }
    }
}

/// A record of a table.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub bytes: Vec<u8>,
    /// What the record can be found by: each a name and a value.
    pub fields: Vec<(String, String)>,
}

impl Record {
    fn size(&self) -> usize {
        let fields = self.fields.iter();
        self.id.len()
            + self.bytes.len()
            + fields.map(|(k, v)| k.len() + v.len()).sum::<usize>()
    }
}

/// Some of the records of a table, in the order of their ids.
#[derive(Clone, PartialEq, Debug)]
pub struct Page {
    pub records: Vec<Record>,
    /// The id to ask after for the next page, when there is one.
    pub next: Option<String>,
}

/// The tables of one plugin, in `folder`. A table is of the bytes
/// `a-z`, `0-9`, `_` and `-`, so it is its own file name and cannot
/// name a place outside the folder. A table is read whole and
/// written whole, to a file beside it that is renamed over it, so it
/// is whole or as it was.
#[derive(Clone, PartialEq, Debug)]
pub struct Db {
    folder: PathBuf,
    caps: Caps,
}

impl Db {
    pub fn new(folder: PathBuf, caps: Caps) -> Self {
        Self { folder, caps }
    }

    /// The record `id` of `table`, if it is there.
    pub fn get(
        &self,
        table: &str,
        id: &str,
    ) -> Result<Option<Record>, String> {
        self.check_table(table)?;
        let records = self.load(table)?;
        Ok(find(&records, id).ok().map(|at| records[at].clone()))
    }

    /// Sets the record of `table` that has the id of `record`,
    /// creating the table if it is not there.
    pub fn put(
        &self,
        table: &str,
        record: Record,
    ) -> Result<(), String> {
        self.check_table(table)?;
        self.check_record(&record)?;
        let mut records = self.load(table)?;
        match find(&records, &record.id) {
            Ok(at) => records[at] = record,
            Err(at) => {
                if records.len() >= self.caps.records {
                    return Err(format!(
                        "the table holds {} records, the most is {}",
                        records.len(),
                        self.caps.records
                    ));
                }
                records.insert(at, record);
            }
        }
        let bytes = postcard::to_allocvec(&records)
            .map_err(|_| "the table could not be encoded")?;
        self.fit(table, bytes.len())?;
        self.write(table, &bytes)
            .map_err(|_| "the table could not be written".to_string())
    }

    /// Removes the record `id` of `table`, if it is there. A table
    /// left with no record is gone.
    pub fn delete(
        &self,
        table: &str,
        id: &str,
    ) -> Result<(), String> {
        self.check_table(table)?;
        let mut records = self.load(table)?;
        let Ok(at) = find(&records, id) else {
            return Ok(());
        };
        records.remove(at);
        let done = match records.is_empty() {
            true => fs::remove_file(self.file(table)),
            false => {
                let bytes = postcard::to_allocvec(&records)
                    .map_err(|_| "the table could not be encoded")?;
                self.write(table, &bytes)
            }
        };
        done.map_err(|_| "the table could not be written".to_string())
    }

    /// A page of `table`, from the first id after `after`, at most
    /// `limit` records.
    pub fn list(
        &self,
        table: &str,
        after: Option<&str>,
        limit: u32,
    ) -> Result<Page, String> {
        self.check_table(table)?;
        let records = self.load(table)?;
        Ok(self.page(after, limit, records, |_| true))
    }

    /// A page of the records of `table` whose field `field` is
    /// `value`, from the first id after `after`, at most `limit`
    /// records.
    pub fn find(
        &self,
        table: &str,
        field: &str,
        value: &str,
        after: Option<&str>,
        limit: u32,
    ) -> Result<Page, String> {
        self.check_table(table)?;
        let records = self.load(table)?;
        Ok(self.page(after, limit, records, |record| {
            record
                .fields
                .iter()
                .any(|(k, v)| k == field && v == value)
        }))
    }

    /// The first records of `records` after `after` that `keep`
    /// holds, within the caps of a page.
    fn page(
        &self,
        after: Option<&str>,
        limit: u32,
        records: Vec<Record>,
        keep: impl Fn(&Record) -> bool,
    ) -> Page {
        let room = self.caps.page_records.min(limit as usize);
        let mut matching = records
            .into_iter()
            .filter(|record| {
                after.is_none_or(|after| record.id.as_str() > after)
            })
            .filter(keep)
            .collect::<Vec<_>>();
        let (mut count, mut bytes) = (0, 0);
        for record in &matching {
            bytes += record.size();
            if count >= room
                || (count > 0 && bytes > self.caps.page_bytes)
            {
                break;
            }
            count += 1;
        }
        let more = count < matching.len();
        matching.truncate(count);
        Page {
            next: matching
                .last()
                .filter(|_| more)
                .map(|last| last.id.clone()),
            records: matching,
        }
    }

    fn file(&self, table: &str) -> PathBuf {
        self.folder.join(format!("{table}{TABLE}"))
    }

    /// Why `table` cannot be a table, if it cannot.
    fn check_table(&self, table: &str) -> Result<(), String> {
        let good = |byte: &u8| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-');
        match table.is_empty()
            || table.len() > self.caps.name_bytes
            || !table.as_bytes().iter().all(good)
        {
            true => Err(format!(
                "`{}` is not a table: 1 to {} bytes of a-z, 0-9, \
                 `_` and `-`",
                table.escape_debug(),
                self.caps.name_bytes
            )),
            false => Ok(()),
        }
    }

    /// Why `record` cannot be kept, if it cannot.
    fn check_record(&self, record: &Record) -> Result<(), String> {
        let caps = &self.caps;
        if record.id.is_empty() || record.id.len() > caps.text_bytes {
            return Err(format!(
                "an id is 1 to {} bytes",
                caps.text_bytes
            ));
        }
        if record.fields.len() > caps.fields {
            return Err(format!(
                "a record has {} fields, the most is {}",
                record.fields.len(),
                caps.fields
            ));
        }
        for (at, (key, value)) in record.fields.iter().enumerate() {
            if key.is_empty()
                || key.len() > caps.name_bytes
                || value.len() > caps.text_bytes
            {
                return Err(format!(
                    "a field is a name of 1 to {} bytes and a value \
                     of up to {}",
                    caps.name_bytes, caps.text_bytes
                ));
            }
            if record.fields[..at].iter().any(|(k, _)| k == key) {
                return Err(format!(
                    "the field `{}` is there twice",
                    key.escape_debug()
                ));
            }
        }
        match record.size() > caps.record_bytes {
            true => Err(format!(
                "a record has {} bytes, the most is {}",
                record.size(),
                caps.record_bytes
            )),
            false => Ok(()),
        }
    }

    /// Whether `table`, written with `bytes`, fits with the others.
    fn fit(&self, table: &str, bytes: usize) -> Result<(), String> {
        let (mut tables, mut others, mut had) = (0, 0, false);
        for (name, size) in self.entries() {
            match name == table {
                true => had = true,
                false => {
                    tables += 1;
                    others += size;
                }
            }
        }
        if !had && tables >= self.caps.tables {
            return Err(format!(
                "the database holds {tables} tables, the most is {}",
                self.caps.tables
            ));
        }
        let after = others + bytes as u64;
        match after > self.caps.bytes {
            true => Err(format!(
                "the database would hold {after} bytes, the most is \
                 {}",
                self.caps.bytes
            )),
            false => Ok(()),
        }
    }

    /// The records of `table`, none where it is not there.
    fn load(&self, table: &str) -> Result<Vec<Record>, String> {
        match fs::read(self.file(table)) {
            Ok(bytes) => postcard::from_bytes(&bytes)
                .map_err(|_| format!("`{table}` is damaged")),
            Err(e) if e.kind() == ErrorKind::NotFound => {
                Ok(Vec::new())
            }
            Err(_) => Err(format!("`{table}` could not be read")),
        }
    }

    /// Each table held, with the bytes of its file.
    fn entries(&self) -> Vec<(String, u64)> {
        let Ok(dir) = fs::read_dir(&self.folder) else {
            return Vec::new();
        };
        dir.filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let table = name.strip_suffix(TABLE)?.to_string();
                let size = entry.metadata().ok()?.len();
                Some((table, size))
            })
            .collect()
    }

    fn write(
        &self,
        table: &str,
        bytes: &[u8],
    ) -> std::io::Result<()> {
        fs::create_dir_all(&self.folder)?;
        let temp = self.folder.join(format!("{WRITING}{table}"));
        let mut file = fs::File::create(&temp)?;
        let written =
            file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        let done = written
            .and_then(|()| fs::rename(&temp, self.file(table)));
        if done.is_err() {
            let _ = fs::remove_file(&temp);
        }
        done
    }
}

/// Where the record `id` is in `records`, or would go.
fn find(records: &[Record], id: &str) -> Result<usize, usize> {
    records.binary_search_by(|record| record.id.as_str().cmp(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("hamlet_db_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn record(id: &str, kind: &str) -> Record {
        Record {
            id: id.into(),
            bytes: id.as_bytes().to_vec(),
            fields: vec![("kind".into(), kind.into())],
        }
    }

    #[test]
    fn records_are_kept_found_paged_and_still_there_for_a_new_db() {
        let root = folder("round");
        let db = Db::new(root.join(DB), Caps::of(&Limits::default()));
        for (id, kind) in [
            ("c", "tea"),
            ("a", "tea"),
            ("b", "coffee"),
            ("d", "tea"),
        ] {
            db.put("items", record(id, kind)).unwrap();
        }
        db.put("items", record("a", "coffee")).unwrap();

        let ids = |page: Page| {
            page.records.into_iter().map(|r| r.id).collect::<Vec<_>>()
        };
        let again =
            Db::new(root.join(DB), Caps::of(&Limits::default()));
        assert_eq!(
            ids(again.list("items", None, 9).unwrap()),
            ["a", "b", "c", "d"]
        );
        let tea =
            again.find("items", "kind", "tea", None, 1).unwrap();
        assert_eq!(tea.next.as_deref(), Some("c"));
        assert_eq!(ids(tea), ["c"]);
        let rest =
            again.find("items", "kind", "tea", Some("c"), 1).unwrap();
        assert_eq!(rest.next, None);
        assert_eq!(ids(rest), ["d"]);

        again.delete("items", "b").unwrap();
        assert_eq!(again.get("items", "b").unwrap(), None);
        assert_eq!(
            again.get("items", "c").unwrap().unwrap().bytes,
            b"c"
        );
        for id in ["a", "c", "d"] {
            again.delete("items", id).unwrap();
        }
        assert!(again.entries().is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_write_over_a_cap_or_under_a_bad_name_is_an_error() {
        let root = folder("caps");
        let caps = Caps {
            bytes: 200,
            tables: 2,
            records: 2,
            record_bytes: 12,
            fields: 1,
            name_bytes: 8,
            text_bytes: 4,
            page_records: 1,
            page_bytes: 100,
        };
        let db = Db::new(root.join(DB), caps);
        db.put("a", record("x", "t")).unwrap();
        db.put("a", record("y", "t")).unwrap();
        assert!(db.put("a", record("z", "t")).is_err(), "records");
        db.put("b", record("x", "t")).unwrap();
        assert!(db.put("c", record("x", "t")).is_err(), "tables");
        let mut big = record("x", "t");
        big.bytes = vec![0; 12];
        assert!(db.put("a", big).is_err(), "record bytes");
        let mut many = record("x", "t");
        many.fields.push(("u".into(), "v".into()));
        assert!(db.put("a", many).is_err(), "fields");
        let mut twice = record("x", "t");
        twice.fields = vec![("k".into(), "1".into()); 2];
        assert!(
            Db::new(root.join(DB), Caps { fields: 2, ..caps })
                .put("a", twice)
                .is_err()
        );
        assert!(db.put("a", record("", "t")).is_err(), "id");
        for table in ["../x", "a/b", "", "Up", "a.b", "toolongname"] {
            assert!(
                db.put(table, record("x", "t")).is_err(),
                "{table}"
            );
            assert!(db.get(table, "x").is_err(), "{table}");
        }
        let page = db.list("a", None, 9).unwrap();
        assert_eq!(
            page.records.len(),
            1,
            "a page holds page_records"
        );
        assert_eq!(page.next.as_deref(), Some("x"));
        assert_eq!(fs::read_dir(root.join(DB)).unwrap().count(), 2);
        let _ = fs::remove_dir_all(&root);
    }
}
