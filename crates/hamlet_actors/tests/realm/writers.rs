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
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let realm = src.join("realm");
    let mut wrong = Vec::new();
    for file in
        rust_files(&realm).into_iter().chain([src.join("realm.rs")])
    {
        let module = file
            .strip_prefix(&realm)
            .unwrap_or(Path::new("realm.rs"))
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&file).unwrap();
        // A test's SQL is data: only the code above it writes.
        let code = text.split("#[cfg(test)]").next().unwrap();
        let allowed = WRITERS
            .iter()
            .find(|(name, _)| *name == module)
            .map_or(&[][..], |(_, tables)| *tables);
        for table in written(code) {
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
    let keywords: Vec<String> =
        words.iter().map(|word| word.to_ascii_uppercase()).collect();
    let mut tables = Vec::new();
    for (i, word) in keywords.iter().enumerate() {
        let after = match word.as_str() {
            "INTO"
                if i > 0
                    && matches!(
                        keywords[i - 1].as_str(),
                        "INSERT" | "REPLACE" | "IGNORE"
                    ) =>
            {
                i + 1
            }
            // Not an upsert's `DO UPDATE SET`, whose table is the
            // insert's.
            "UPDATE"
                if keywords.get(i + 1).map(String::as_str)
                    != Some("SET") =>
            {
                i + 1
            }
            "FROM" if i > 0 && keywords[i - 1] == "DELETE" => i + 1,
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

#[test]
fn the_scan_reads_a_write_in_either_case() {
    assert_eq!(
        written(
            "insert into agent (n) VALUES (1) ON CONFLICT (n) DO \
             UPDATE SET n = 1; update hire SET n = 2; DELETE from \
             step"
        ),
        ["agent", "hire", "step"]
    );
}
