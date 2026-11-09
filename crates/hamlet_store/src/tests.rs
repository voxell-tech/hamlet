use std::fmt::Write;
use std::path::Path;

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use hamlet_testing::Scratch;
use regex::Regex;

use crate::marker::{self, Identity, Marked, Request};
use crate::{Store, Tx, job};

/// The schema module, from the crate's root.
const SCHEMA_RS: &str = "src/schema.rs";

#[derive(QueryableByName)]
struct Table {
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Text)]
    sql: String,
}

#[derive(QueryableByName)]
struct Column {
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Text)]
    kind: String,
    #[diesel(sql_type = BigInt)]
    notnull: i64,
    #[diesel(sql_type = BigInt)]
    pk: i64,
}

#[derive(QueryableByName)]
struct ForeignKey {
    #[diesel(sql_type = BigInt)]
    id: i64,
    #[diesel(sql_type = Text)]
    parent: String,
    #[diesel(sql_type = Text)]
    child: String,
    #[diesel(sql_type = Text)]
    target: String,
}

fn columns_of(db: &mut SqliteConnection, table: &str) -> Vec<Column> {
    diesel::sql_query(
        "SELECT name, type AS kind, \"notnull\", pk FROM \
         pragma_table_info(?)",
    )
    .bind::<Text, _>(table)
    .load(db)
    .unwrap()
}

/// `schema.rs` for the tables of `db`.
fn module(db: &mut SqliteConnection) -> String {
    let tables = diesel::sql_query(
        "SELECT name, sql FROM sqlite_schema WHERE type = 'table' \
         ORDER BY name",
    )
    .load::<Table>(db)
    .unwrap();
    let mut out = String::from(
        "// Generated from `schema.sql` by its test. Do not \
         edit.\n\n",
    );
    let mut joins = Vec::<(String, String, String)>::new();
    for table in &tables {
        let columns = columns_of(db, &table.name);
        let mut keys =
            columns.iter().filter(|c| c.pk > 0).collect::<Vec<_>>();
        keys.sort_by_key(|c| c.pk);
        assert!(
            !keys.is_empty(),
            "{} has no primary key",
            table.name
        );
        let keys = keys
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            out,
            "diesel::table! {{\n    {} ({keys}) {{",
            table.name
        )
        .unwrap();
        for column in &columns {
            let name = regex::escape(&column.name);
            let boolean = Regex::new(&format!(
                r"\b{name}\b INTEGER[^,\n]*CHECK \(({name} IS NULL OR )?{name} IN \(0, 1\)\)"
            ))
            .unwrap()
            .is_match(&table.sql);
            let base = match column.kind.as_str() {
                _ if boolean => "Bool",
                "INTEGER" => "BigInt",
                "TEXT" => "Text",
                "BLOB" => "Binary",
                "REAL" => "Double",
                other => panic!("no Diesel type for {other}"),
            };
            let kind = if column.notnull != 0 || column.pk > 0 {
                base.to_owned()
            } else {
                format!("Nullable<{base}>")
            };
            writeln!(out, "        {} -> {kind},", column.name)
                .unwrap();
        }
        out.push_str("    }\n}\n\n");
        let keys = diesel::sql_query(
            "SELECT id, \"table\" AS parent, \"from\" AS child, \
             \"to\" AS target FROM pragma_foreign_key_list(?) ORDER \
             BY \"table\", id",
        )
        .bind::<Text, _>(&table.name)
        .load::<ForeignKey>(db)
        .unwrap();
        let mut parents = Vec::<(&str, Vec<&str>)>::new();
        for key in &keys {
            let single =
                keys.iter().filter(|k| k.id == key.id).count() == 1;
            let target = columns_of(db, &key.parent)
                .into_iter()
                .filter(|c| c.pk > 0)
                .map(|c| c.name)
                .collect::<Vec<_>>();
            if !single
                || target != [key.target.as_str()]
                || key.parent == table.name
            {
                continue;
            }
            match parents.iter_mut().find(|(p, _)| *p == key.parent) {
                Some((_, from)) => from.push(&key.child),
                None => parents.push((&key.parent, vec![&key.child])),
            }
        }
        for (parent, mut from) in parents {
            // Diesel allows one join per pair of tables: the column
            // named after the parent when there are several, else
            // none.
            if from.len() > 1 {
                from.retain(|f| *f == parent);
            }
            if let [from] = from[..] {
                joins.push((
                    table.name.clone(),
                    parent.to_owned(),
                    from.to_owned(),
                ));
            }
        }
    }
    // A join also goes the other way, so of two tables joined both
    // ways only the one through a column named after its parent
    // stays.
    let kept = joins
        .iter()
        .filter(|(child, parent, from)| {
            let back = joins
                .iter()
                .find(|(c, p, _)| c == parent && p == child);
            back.is_none_or(|(_, p, f)| {
                from == parent && f != p
                    || (from == parent) == (f == p) && child < parent
            })
        })
        .collect::<Vec<_>>();
    for (child, parent, from) in kept {
        writeln!(
            out,
            "diesel::joinable!({child} -> {parent} ({from}));"
        )
        .unwrap();
    }
    out.push_str(
        "\ndiesel::allow_tables_to_appear_in_same_query!(\n",
    );
    for table in &tables {
        writeln!(out, "    {},", table.name).unwrap();
    }
    out.push_str(");\n");
    out
}

fn store(dir: &Scratch) -> Store {
    Store::create(&dir.join("realm.sqlite"), "realm-1", 1).unwrap()
}

#[test]
fn the_schema_module_is_current() {
    let dir = Scratch::new("store-schema");
    let expected = module(store(&dir).connection());
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(SCHEMA_RS);
    // A Windows checkout may end its lines with CRLF.
    if std::fs::read_to_string(&path)
        .is_ok_and(|kept| kept.replace("\r\n", "\n") == expected)
    {
        return;
    }
    std::fs::write(&path, expected).unwrap();
    panic!(
        "{SCHEMA_RS} was stale and is regenerated from the schema; \
         commit it."
    );
}

#[test]
fn a_realm_opens_only_as_its_own_database() {
    let dir = Scratch::new("store-open");
    let path = dir.join("realm.sqlite");
    assert!(Store::open(&path).is_err(), "a missing database opens");
    let mut created = store(&dir);
    assert_eq!(created.realm().unwrap(), "realm-1");
    drop(created);
    assert!(Store::create(&path, "realm-2", 1).is_err());
    assert_eq!(
        Store::open(&path).unwrap().realm().unwrap(),
        "realm-1"
    );

    let other = dir.join("other.sqlite");
    let mut db =
        SqliteConnection::establish(other.to_str().unwrap()).unwrap();
    diesel::sql_query("CREATE TABLE t (x INTEGER)")
        .execute(&mut db)
        .unwrap();
    drop(db);
    assert!(Store::open(&other).is_err(), "another database opens");
}

#[test]
fn a_marker_answers_its_request_and_refuses_another() {
    let dir = Scratch::new("store-marker");
    let mut store = store(&dir);
    let id = Identity {
        source: "pi-1",
        key: "call-1",
    };
    let request = Request::new("tool", &("write", 1)).unwrap();
    let other = Request::new("tool", &("write", 2)).unwrap();
    let mut tx = store.transaction().unwrap();
    assert_eq!(
        marker::read::<u32>(&mut tx, id, &request).unwrap(),
        Marked::Unmarked
    );
    marker::begin(&mut tx, id, &request, 1).unwrap();
    assert_eq!(
        marker::read::<u32>(&mut tx, id, &request).unwrap(),
        Marked::Begun
    );
    marker::settle(&mut tx, id, &request, &7_u32, 2).unwrap();
    tx.commit().unwrap();
    let db = store.connection();
    assert_eq!(
        marker::read::<u32>(db, id, &request).unwrap(),
        Marked::Outcome(7)
    );
    assert_eq!(
        marker::read::<u32>(db, id, &other).unwrap(),
        Marked::Conflict
    );
}

#[test]
fn a_keyed_job_waits_once_and_is_done_once() {
    let dir = Scratch::new("store-job");
    let mut store = store(&dir);
    let mut tx: Tx<'_> = store.transaction().unwrap();
    let first =
        job::add(&mut tx, "commit", Some("commit:4"), &4, 1).unwrap();
    let again =
        job::add(&mut tx, "commit", Some("commit:4"), &4, 2).unwrap();
    assert_eq!(first, again);
    let plain = job::add(&mut tx, "erase", None, &"x", 3).unwrap();
    job::retry(&mut tx, plain, "busy", 100).unwrap();
    tx.commit().unwrap();
    let db = store.connection();
    let due = job::due(db, 50).unwrap();
    assert_eq!(due.iter().map(|j| j.id).collect::<Vec<_>>(), [first]);
    assert_eq!(job::next(db).unwrap(), Some(100));
    job::finish(db, first, 60).unwrap();
    let due = job::due(db, 100).unwrap();
    assert_eq!(due.iter().map(|j| j.id).collect::<Vec<_>>(), [plain]);
    assert_eq!(due[0].tries, 1);
    let later =
        job::add(db, "commit", Some("commit:4"), &4, 101).unwrap();
    assert_ne!(later, first, "a done job's key is free again");
}
