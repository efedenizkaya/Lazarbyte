//! Owning a file descriptor handed over by the Kotlin side.
//!
//! This is the one approved unsafe exception in the project (AGENTS.md §3,
//! §10). It exists because Android's Storage Access Framework hands out file
//! descriptors, not paths: the app opens a document with
//! `ContentResolver.openFileDescriptor(..., "r")`, calls
//! `ParcelFileDescriptor.detachFd()` and passes the number across the bridge.
//!
//! # Safety contract
//!
//! `source_from_fd` takes ownership of `fd` by turning it into an `OwnedFd`,
//! which closes it on drop. The call is sound if and only if:
//!
//! 1. the descriptor is an open, readable file descriptor, and
//! 2. the Kotlin side has detached it (`detachFd()`), so it is the sole owner:
//!    nothing else will read or close it while the scan runs, and Kotlin will
//!    not close it afterwards.
//!
//! The Kotlin side of this contract lives where the descriptor is detached
//! (see the source-selection code in the app). The descriptor is used read-
//! only from here on: `Source` never writes to what it opens, whatever it
//! wraps. A violation of (1) or (2) is a bug in the caller, not in this file;
//! a bad number from the foreign side cannot corrupt memory by itself, it can
//! only fail to open or read from the wrong file.

#[cfg(unix)]
#[allow(unsafe_code)]
pub(crate) fn source_from_fd(fd: i32) -> std::io::Result<lazarbyte_core::reader::Source> {
    use std::os::fd::FromRawFd;

    // SAFETY: the contract above; Kotlin detached the descriptor, so the
    // number crosses the boundary with its ownership attached.
    let owned = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
    lazarbyte_core::reader::Source::from_file(std::fs::File::from(owned), &format!("fd:{fd}"))
}
