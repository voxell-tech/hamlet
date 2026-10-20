//! A home opened by this server: claimed, laid out and with its store
//! open.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use rusqlite::OptionalExtension;

use crate::claim::Claim;
use crate::operation::RuntimeInstance;
use crate::store::Store;

/// The realm database, which only this server opens.
const STORE: &str = "realm.sqlite";
/// The home's identity, also kept in its realm database.
const IDENTITY: &str = "realm.id";
/// The folder of the harness runtimes' own storage.
const HARNESSES: &str = "harnesses";
/// The credentials file, which only this server writes.
const CREDENTIALS: &str = "credentials.json";

/// The paths of a home's parts.
#[derive(Clone, Debug)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The canonical home folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The realm database's path.
    pub fn store(&self) -> PathBuf {
        self.root.join(STORE)
    }

    /// The model credentials and the connection's token.
    pub fn credentials(&self) -> PathBuf {
        self.root.join(CREDENTIALS)
    }

    /// The folder only `instance` keeps its storage in.
    pub fn harness(&self, instance: &RuntimeInstance) -> PathBuf {
        self.root.join(HARNESSES).join(instance.as_str())
    }
}

/// A home this process holds, with its store open.
pub struct Home {
    layout: Layout,
    // Declared before the claim, so a dropped home closes its store
    // before it lets the home go.
    store: Store,
    _claim: Claim,
}

impl Home {
    /// Claims the home in `dir`, creating the folder when missing,
    /// then opens its store.
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        let root = dir.canonicalize().with_context(|| {
            format!("resolving {}", dir.display())
        })?;
        let claim = Claim::take(&root)?;
        let layout = Layout::new(root);
        ensure!(
            layout.store().try_exists()?
                || !layout.root.join(IDENTITY).try_exists()?,
            "the realm database is missing; restore the complete \
             home from backup"
        );
        let mut store = Store::open(&layout.store())?;
        identify(&layout, &mut store)?;
        Ok(Self {
            layout,
            store,
            _claim: claim,
        })
    }

    /// The paths of this home's parts.
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The claimed home's realm store.
    pub fn store(&mut self) -> &mut Store {
        &mut self.store
    }

    /// Closes the store, then lets the home go.
    pub fn close(self) -> Result<()> {
        self.store.close()
    }
}

/// Checks the identity pair, or creates it for a fresh home.
fn identify(layout: &Layout, store: &mut Store) -> Result<()> {
    let path = layout.root.join(IDENTITY);
    let tx = store.transaction()?;
    let identity = tx
        .query_row(
            "SELECT home FROM store_identity WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(identity) = identity {
        let kept = std::fs::read_to_string(&path).context(
            "reading realm.id; restore the complete home from \
             backup if it is missing",
        )?;
        ensure!(
            kept == identity,
            "the realm database belongs to another home; restore \
             the matching store and realm.id from a complete backup"
        );
    } else {
        let identity = uuid::Uuid::new_v4().to_string();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .context(
                "creating realm.id; an existing identity without \
                 its database identity needs a complete home restore",
            )?;
        file.write_all(identity.as_bytes())?;
        file.sync_all()?;
        #[cfg(unix)]
        std::fs::File::open(&layout.root)?.sync_all()?;
        // A failure before commit leaves an incomplete pair, which
        // reopening refuses.
        tx.execute(
            "INSERT INTO store_identity (id, home) VALUES (1, ?1)",
            [&identity],
        )?;
    }
    tx.commit()?;
    Ok(())
}
