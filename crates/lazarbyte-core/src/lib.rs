//! Signature-based file carver for disk images and block devices.
//!
//! A Rust port of the carving core of BreadCrumb (https://github.com/sltcnb/BreadCrumb):
//! scan raw bytes for file headers, parse each candidate's structure to find
//! its true end, and write the recovered bytes out with a SHA-256 manifest.
//! No filesystem metadata is needed or consulted.

// The core crate handles untrusted image bytes. It must stay free of unsafe
// code; any FFI or raw file-descriptor work belongs in lazarbyte-ffi instead.
#![forbid(unsafe_code)]

#[cfg(feature = "apfs")]
pub mod apfs;
#[cfg(feature = "artifacts")]
pub mod artifacts;
#[cfg(feature = "bitlocker")]
pub mod bitlocker;
pub mod carver;
pub mod checkpoint;
#[cfg(feature = "bitlocker")]
pub mod crypto;
#[cfg(feature = "customsig")]
pub mod customsig;
#[cfg(feature = "ewf")]
pub mod ewf;
pub mod ext4;
pub mod fat;
pub mod freespace;
#[cfg(feature = "grep")]
pub mod grep;
pub mod handlers;
#[cfg(feature = "hfs")]
pub mod hfs;
#[cfg(feature = "containers")]
pub mod images;
pub mod json;
#[cfg(feature = "jsonin")]
pub mod jsonin;
#[cfg(feature = "ntfs")]
pub mod ntfs;
pub mod partition;
pub mod reader;
pub mod recover;
pub mod report;
pub mod signatures;
pub mod validate;
pub mod verify;
pub mod window;
