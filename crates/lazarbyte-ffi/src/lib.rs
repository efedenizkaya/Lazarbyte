//! uniffi bindings over `lazarbyte-core`.
//!
//! The engines live in `lazarbyte-core`; this crate only adapts them to the
//! types uniffi can cross the FFI boundary with. Business logic stays out: an
//! exported function parses arguments, calls one core entry point and maps its
//! `Result` to an error the foreign side can catch.
//!
//! The whole crate is unsafe-free. The FdSource that will wrap a Kotlin
//! `ParcelFileDescriptor` is the one approved exception to that rule
//! (AGENTS.md §10) and lands with the Android build, not before.

// Proc-macro-only API: no UDL file, no build.rs (the current official
// recommendation, uniffi 0.32).
uniffi::setup_scaffolding!();

/// Proof the bridge links, and the version the app can show. The first of the
/// exported surface; the engines follow in the next steps.
#[uniffi::export]
pub fn bridge_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
