//! A home opened by this server: claimed, laid out and with its store
//! open.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::claim::Claim;
use crate::operation::RuntimeInstance;
use crate::store::Store;

/// The realm database, which only this server opens.
const STORE: &str = "realm.sqlite";
/// The folder of the harness runtimes' own storage.
const HARNESSES: &str = "harnesses";
/// The TypeScript server's storage.
const NODE_STORE: &str = "home.sqlite";

/// The paths of a home's parts.
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn store(&self) -> PathBuf {
        self.root.join(STORE)
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
    /// then opens and migrates its store.
    pub fn open(dir: &Path) -> Result<Self> {
        let root = std::path::absolute(dir)?;
        std::fs::create_dir_all(&root).with_context(|| {
            format!("creating {}", root.display())
        })?;
        let claim = Claim::take(&root)?;
        let layout = Layout::new(root);
        if !layout.store().exists()
            && layout.root.join(NODE_STORE).exists()
        {
            bail!(
                "{} holds the TypeScript server's storage, which \
                 this server cannot open.",
                layout.root.display()
            );
        }
        let store = Store::open(&layout.store())?;
        Ok(Self {
            layout,
            store,
            _claim: claim,
        })
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn store(&mut self) -> &mut Store {
        &mut self.store
    }

    /// Closes the store, then lets the home go.
    pub fn close(self) -> Result<()> {
        self.store.close()
    }
}
