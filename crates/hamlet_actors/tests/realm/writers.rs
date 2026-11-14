//! Each table of `schema.sql` has one writer: the module named above
//! it. The check reads the realm's source, so a change that writes a
//! table from another module fails here, naming the module and the
//! table, before review.

use std::path::Path;

/// Each realm module that writes rows, and the tables it may write.
const WRITERS: &[(&str, &[&str])] = &[
    ("chiefs.rs", &["realm", "chief", "hamlet", "hamlet_chief"]),
    ("chiefs/limits.rs", &["realm", "hamlet"]),
    ("chiefs/tier.rs", &["hamlet"]),
    ("tree.rs", &["profile", "agent", "hire"]),
    ("tree/lead.rs", &["agent", "hire"]),
    ("tree/tools.rs", &["hire_tool_off"]),
    ("versions.rs", &["reach", "place", "revision"]),
    ("marker.rs", &["marker"]),
    ("record.rs", &["decision"]),
    ("steps.rs", &["step"]),
    ("tasks.rs", &["task"]),
    ("letters.rs", &["letter"]),
    ("laid.rs", &["road"]),
    ("budget.rs", &["charge", "budget_reset"]),
    ("waits.rs", &["wait"]),
];

#[test]
fn each_table_has_one_writer() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/realm");
    let mut wrong = Vec::new();
    for file in rust_files(&src) {
        let module = file
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&file).unwrap();
        let allowed = WRITERS
            .iter()
            .find(|(name, _)| *name == module)
            .map_or(&[][..], |(_, tables)| *tables);
        for table in written(&text) {
            if !allowed.contains(&table.as_str()) {
                wrong.push(format!("{module} writes `{table}`"));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "a table has another writer:\n{}",
        wrong.join("\n")
    );
}

/// The tables `text` inserts into, updates or deletes from.
fn written(text: &str) -> Vec<String> {
    let words: Vec<&str> = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .collect();
    let mut tables = Vec::new();
    for (i, word) in words.iter().enumerate() {
        let after = match *word {
            "INTO"
                if i > 0
                    && matches!(
                        words[i - 1],
                        "INSERT" | "REPLACE" | "IGNORE"
                    ) =>
            {
                i + 1
            }
            // Not an upsert's `DO UPDATE SET`, whose table is the
            // insert's.
            "UPDATE" if words.get(i + 1) != Some(&"SET") => i + 1,
            "FROM" if i > 0 && words[i - 1] == "DELETE" => i + 1,
            _ => continue,
        };
        if let Some(table) = words.get(after) {
            tables.push(table.to_string());
        }
    }
    tables
}

fn rust_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files
}
