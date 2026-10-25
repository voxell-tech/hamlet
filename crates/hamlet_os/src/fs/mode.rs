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

    /// `bits` without setuid, setgid and sticky.
    pub fn new(bits: u32) -> Self {
        Self(bits & 0o777)
    }

    pub fn bits(self) -> u32 {
        self.0
    }
}
