use super::Kind;

/// POSIX permission bits, always within `0o777`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Mode(u32);

impl Mode {
    /// A folder the server makes.
    pub const FOLDER: Self = Self(0o700);
    /// A file only the server reads.
    pub const PRIVATE: Self = Self(0o600);
    /// A file anyone on the machine may read.
    pub const SHARED: Self = Self(0o644);
    /// A file no one may write.
    pub(crate) const READ_ONLY: Self = Self(0o444);

    /// `bits` without setuid, setgid and sticky.
    pub const fn new(bits: u32) -> Self {
        Self(bits & 0o777)
    }

    pub fn bits(self) -> u32 {
        self.0
    }

    /// What this mode reads back as once set on a `kind` here. Unix
    /// keeps every bit. Windows keeps only whether a file is
    /// read-only: a file reads back as `0o444` without the owner's
    /// write bit and as `0o644` with it, and a folder as `0o700`.
    pub fn kept(self, kind: Kind) -> Self {
        if !cfg!(windows) {
            return self;
        }
        match kind {
            Kind::Directory => Self::FOLDER,
            _ if self.0 & 0o200 == 0 => Self::READ_ONLY,
            _ => Self::SHARED,
        }
    }
}
