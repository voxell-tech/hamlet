use std::path::Path;

/// Each realm module that writes rows, and the tables it may write.
const WRITERS: &[(&str, &[&str])] = &[
    ("chiefs.rs", &["realm", "chief", "hamlet", "hamlet_chief"]),
    ("chiefs/hamlet.rs", &["hamlet", "hamlet_folder"]),
    ("chiefs/limits.rs", &["realm", "hamlet"]),
    ("chiefs/tier.rs", &["hamlet"]),
    ("profiles.rs", &["profile"]),
    ("tree.rs", &["agent", "hire"]),
    ("tree/choices.rs", &["nickname", "model_choice"]),
    ("tree/lead.rs", &["agent", "hire"]),
    ("tree/tools.rs", &["hire_tool_off"]),
    ("versions.rs", &["reach", "place", "revision", "copy"]),
    ("marker.rs", &["marker"]),
    ("record.rs", &["decision"]),
    ("steps.rs", &["step"]),
    ("tasks.rs", &["task"]),
    ("letters.rs", &["letter"]),
    ("laid.rs", &["road"]),
    ("budget.rs", &["charge", "budget_reset"]),
    ("waits.rs", &["wait"]),
    ("oversight.rs", &["grant", "trust"]),
    ("oversight/asking.rs", &["objection", "held"]),
    ("workers.rs", &["worker"]),
    ("workers/errands.rs", &["errand"]),
    ("reviews.rs", &["review"]),
    ("allowance.rs", &["allowance"]),
    ("society.rs", &["agreement"]),
    ("documents.rs", &["document"]),
    ("settings.rs", &["settings"]),
    ("parcels.rs", &["parcel", "parcel_file"]),
    ("dreams.rs", &["dream"]),
    ("librarian.rs", &["librarian"]),
    ("newcomers.rs", &["newcomer"]),
    ("merges.rs", &["merge"]),
];

/// The handlers that run outside `Act`, by module: each writes
/// something other than rows before it commits, reads only something
/// other than rows, or reads what an act's steps did once it
/// answered.
const OUTSIDE_ACT: &[(&str, &str)] = &[
    ("secrets.rs", "KeepCredential"),
    ("secrets.rs", "RememberSecret"),
    ("settings.rs", "Asked"),
    ("parcels.rs", "FetchPart"),
    ("parcels.rs", "Measuring"),
    ("profiles/making.rs", "Create"),
    ("profiles/making.rs", "ChiefCreate"),
    ("profiles.rs", "Purging"),
    ("profiles.rs", "Asked"),
    ("dreams.rs", "ChiefDream"),
    ("librarian.rs", "Submitting"),
    ("tree/lead.rs", "Pinning"),
    ("chiefs.rs", "Pinning"),
    ("lines.rs", "Working"),
    ("stop.rs", "StopAgents"),
    ("on_task.rs", "Asking"),
    ("profiles/lifecycle.rs", "Forking"),
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
        let code = without_test_items(&text);
        let allowed = WRITERS
            .iter()
            .find(|(name, _)| *name == module)
            .map_or(&[][..], |(_, tables)| *tables);
        for table in written(&code) {
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

#[test]
fn each_handler_outside_act_is_named() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let realm = src.join("realm");
    let mut unnamed = Vec::new();
    for file in rust_files(&realm) {
        let module = file
            .strip_prefix(&realm)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if module == "callers.rs" {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        let code = without_test_items(&text);
        for (at, _) in code.match_indices("Handler<") {
            let handled = code[at + "Handler<".len()..]
                .split(|c: char| {
                    !(c.is_ascii_alphanumeric() || c == '_')
                })
                .next()
                .unwrap_or_default();
            if !OUTSIDE_ACT.contains(&(module.as_str(), handled)) {
                unnamed.push(format!("{module} handles `{handled}`"));
            }
        }
    }
    assert!(
        unnamed.is_empty(),
        "a handler runs outside `Act` unnamed:\n{}",
        unnamed.join("\n")
    );
}

fn without_test_items(text: &str) -> String {
    let mut code = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("#[cfg(test)]") {
        code.push_str(&rest[..at]);
        let item = &rest[at..];
        let end = match (item.find(';'), item.find('{')) {
            (Some(semi), Some(open)) if semi < open => semi + 1,
            (Some(semi), None) => semi + 1,
            (_, Some(open)) => open + block_len(&item[open..]),
            (None, None) => item.len(),
        };
        rest = &item[end..];
    }
    code + rest
}

fn block_len(text: &str) -> usize {
    let mut depth = 0;
    for (i, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 1 => return i + 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    text.len()
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
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && !path.ends_with("tests.rs")
        {
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

#[test]
fn the_scan_skips_only_test_items() {
    let text =
        "#[cfg(test)]\nuse std::fmt as _;\nconst A: &str = \"INSERT \
         INTO agent (n) VALUES (1)\";\n#[cfg(test)]\nfn seed() { \
         \"DELETE FROM step\"; }\n#[cfg(test)]\nmod testing { fn \
         f() { if true { \"UPDATE hire SET n = 1\"; } } }\nconst B: \
         &str = \"DELETE FROM task\";\n#[cfg(test)]\nmod tests;\n";
    assert_eq!(written(&without_test_items(text)), ["agent", "task"]);
}
