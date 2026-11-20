use sha1::{Digest, Sha1};
use sha2::{Sha256, Sha512};

/// Digests supplied by trusted upstream metadata. SHA-1 is for legacy
/// interoperability. Prefer SHA-256 or SHA-512 when both are available.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Checksum {
    Sha1([u8; 20]),
    Sha256([u8; 32]),
    Sha512([u8; 64]),
}

impl Checksum {
    pub(super) fn hasher(&self) -> Hasher {
        match self {
            Self::Sha1(_) => Hasher::Sha1(Sha1::new()),
            Self::Sha256(_) => Hasher::Sha256(Sha256::new()),
            Self::Sha512(_) => Hasher::Sha512(Sha512::new()),
        }
    }
}

pub(super) enum Hasher {
    Sha1(Sha1),
    Sha256(Sha256),
    Sha512(Sha512),
}

impl Hasher {
    pub(super) fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha1(hash) => hash.update(data),
            Self::Sha256(hash) => hash.update(data),
            Self::Sha512(hash) => hash.update(data),
        }
    }

    pub(super) fn matches(self, checksum: &Checksum) -> bool {
        match (self, checksum) {
            (Self::Sha1(hash), Checksum::Sha1(expected)) => &hash.finalize()[..] == expected,
            (Self::Sha256(hash), Checksum::Sha256(expected)) => &hash.finalize()[..] == expected,
            (Self::Sha512(hash), Checksum::Sha512(expected)) => &hash.finalize()[..] == expected,
            _ => false,
        }
    }
}
