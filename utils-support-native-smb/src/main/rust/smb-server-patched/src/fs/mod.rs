//! Local-filesystem [`ShareBackend`] for `smb-server`, sandboxed via `cap-std`.

#[cfg(feature = "localfs")]
mod local;

#[cfg(feature = "localfs")]
pub use local::LocalFsBackend;
