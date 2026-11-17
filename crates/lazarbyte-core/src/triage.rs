//! Post-scan triage over a set of recovered-file records.
//!
//! The engines report per file; an examination needs the shape of the whole
//! result -- how many of each kind, what is worth looking at first, which
//! rows are byte-identical copies of each other, and the time span the
//! deleted files were last modified. This module computes that shape from
//! normalized rows, so the CLI, the FFI bridge and the tests all share one
//! definition of "the summary".
//!
//! Everything here is arithmetic over data the caller already read: no
//! source access, no panics, `checked`/`saturating` arithmetic on sums.

/// One recovered file, normalized across engines.
pub struct TriageRow {
    /// The engine's own family name: jpg, fat, exfat, ext4...
    pub kind: String,
    pub name: String,
    pub ext: String,
    pub size: u64,
    pub sha256: String,
    /// The declared length was readable inside the volume.
    pub validated: bool,
    /// Last modification time of the recovered file, when the engine knows
    /// it (undelete modes do; carved files carry no timestamps). Epoch
    /// seconds, zero means unknown.
    pub modified: Option<u64>,
}

/// Files per format family, sorted by bytes.
pub struct KindCount {
    pub kind: String,
    pub files: u64,
    pub bytes: u64,
}

/// One of the biggest recovered files.
pub struct LargestFile {
    pub name: String,
    pub kind: String,
    pub size: u64,
}

/// The shape of a scan result.
pub struct TriageSummary {
    pub total_files: u64,
    pub total_bytes: u64,
    /// Per format family, largest total first.
    pub by_kind: Vec<KindCount>,
    /// Up to the five biggest files.
    pub largest: Vec<LargestFile>,
    /// SHA-256 values that appear on more than one row: byte-identical
    /// files found by different engines or at different offsets.
    pub duplicate_sha256: Vec<String>,
    /// How many rows share a duplicated SHA-256 (one row keeps its copy,
    /// the rest are candidates for the caller to hide or merge).
    pub duplicate_rows: u64,
    /// Rows whose extension is not one the signature table knows: a hint
    /// that the recovered name may not match the recovered bytes.
    pub unknown_extensions: u64,
    /// The modified-time span over rows that carry a time; `None` when no
    /// row does (a carve-only result has no timestamps).
    pub modified_min: Option<u64>,
    pub modified_max: Option<u64>,
    /// Rows the engines could not fully account for.
    pub unvalidated: u64,
}

/// Compute the summary. `known_extensions` is the set of extensions the
/// signature table recognises (lower-case, without dots); the caller builds
/// it from the signature table so this module stays decoupled from it.
pub fn summarize(
    rows: &[TriageRow],
    known_extensions: &std::collections::HashSet<String>,
) -> TriageSummary {
    let total_files = rows.len() as u64;
    let total_bytes = rows.iter().map(|r| r.size).fold(0u64, u64::saturating_add);

    // Per-kind counts, then largest-total-first ordering.
    let mut kinds: std::collections::BTreeMap<String, (u64, u64)> = Default::default();
    let mut modified_min: Option<u64> = None;
    let mut modified_max: Option<u64> = None;
    let mut unvalidated = 0u64;
    for row in rows {
        let entry = kinds.entry(row.kind.clone()).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.saturating_add(row.size);
        if let Some(m) = row.modified.filter(|m| *m != 0) {
            modified_min = Some(modified_min.map_or(m, |cur: u64| cur.min(m)));
            modified_max = Some(modified_max.map_or(m, |cur: u64| cur.max(m)));
        }
        if !row.validated {
            unvalidated = unvalidated.saturating_add(1);
        }
    }
    let mut by_kind: Vec<KindCount> = kinds
        .into_iter()
        .map(|(kind, (files, bytes))| KindCount { kind, files, bytes })
        .collect();
    by_kind.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.kind.cmp(&b.kind)));

    // Biggest files, top five.
    let mut largest_rows: Vec<&TriageRow> = rows.iter().collect();
    largest_rows.sort_by_key(|r| std::cmp::Reverse(r.size));
    let largest: Vec<LargestFile> = largest_rows
        .into_iter()
        .take(5)
        .map(|r| LargestFile {
            name: r.name.clone(),
            kind: r.kind.clone(),
            size: r.size,
        })
        .collect();

    // SHA-256 repetition: the same bytes found twice.
    let mut counts: std::collections::HashMap<&str, u64> = Default::default();
    for row in rows {
        counts
            .entry(row.sha256.as_str())
            .and_modify(|c| *c = c.saturating_add(1))
            .or_insert(1);
    }
    let mut duplicate_sha256: Vec<String> = counts
        .iter()
        .filter(|(_, c)| **c > 1)
        .map(|(sha, _)| (*sha).to_string())
        .collect();
    duplicate_sha256.sort();
    let duplicate_rows = rows
        .iter()
        .filter(|r| counts.get(r.sha256.as_str()).copied().unwrap_or(0) > 1)
        .count() as u64;

    // Extension recognition: an unknown extension on a recovered name is a
    // hint, not a verdict -- the row may simply be a type the table lacks.
    let mut unknown_extensions = 0u64;
    for row in rows {
        let ext = row.ext.to_ascii_lowercase();
        if !ext.is_empty() && !known_extensions.contains(&ext) {
            unknown_extensions = unknown_extensions.saturating_add(1);
        }
    }

    TriageSummary {
        total_files,
        total_bytes,
        by_kind,
        largest,
        duplicate_sha256,
        duplicate_rows,
        unknown_extensions,
        modified_min,
        modified_max,
        unvalidated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn row(
        kind: &str,
        name: &str,
        ext: &str,
        size: u64,
        sha: &str,
        modified: Option<u64>,
    ) -> TriageRow {
        TriageRow {
            kind: kind.into(),
            name: name.into(),
            ext: ext.into(),
            size,
            sha256: sha.into(),
            validated: true,
            modified,
        }
    }

    fn known() -> HashSet<String> {
        ["jpg", "png", "txt", "bin"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn counts_kinds_bytes_and_largest() {
        let rows = vec![
            row("jpg", "a.jpg", "jpg", 100, "sha-a", None),
            row("jpg", "b.jpg", "jpg", 50, "sha-b", None),
            row("txt", "c.txt", "txt", 10, "sha-c", None),
        ];
        let t = summarize(&rows, &known());
        assert_eq!(t.total_files, 3);
        assert_eq!(t.total_bytes, 160);
        assert_eq!(t.by_kind.len(), 2);
        assert_eq!(t.by_kind[0].kind, "jpg");
        assert_eq!(t.by_kind[0].bytes, 150);
        assert_eq!(t.largest.len(), 3);
        assert_eq!(t.largest[0].name, "a.jpg");
        assert_eq!(t.largest[0].size, 100);
    }

    #[test]
    fn byte_identical_rows_are_reported_as_duplicates() {
        let rows = vec![
            row(
                "fat",
                "deleted.txt",
                "txt",
                880,
                "sha-dup",
                Some(1700000000),
            ),
            row("jpg", "f_1000.jpg", "jpg", 880, "sha-dup", None),
            row(
                "fat",
                "inner.bin",
                "bin",
                5120,
                "sha-unique",
                Some(1700001000),
            ),
        ];
        let t = summarize(&rows, &known());
        assert_eq!(t.duplicate_sha256, vec!["sha-dup".to_string()]);
        assert_eq!(t.duplicate_rows, 2);
    }

    #[test]
    fn modified_range_ignores_rows_without_time() {
        let rows = vec![
            row("fat", "a", "txt", 1, "s1", Some(1700001000)),
            row("fat", "b", "txt", 1, "s2", Some(1700000000)),
            row("jpg", "c", "jpg", 1, "s3", None),
            row("fat", "d", "txt", 1, "s4", Some(0)), // unknown, ignored
        ];
        let t = summarize(&rows, &known());
        assert_eq!(t.modified_min, Some(1700000000));
        assert_eq!(t.modified_max, Some(1700001000));
    }

    #[test]
    fn unknown_extensions_are_counted_and_unvalidated_rows_tracked() {
        let mut r = row("jpg", "weird.xyz", "xyz", 10, "s1", None);
        r.validated = false;
        let rows = vec![r, row("txt", "a.txt", "txt", 10, "s2", None)];
        let t = summarize(&rows, &known());
        assert_eq!(t.unknown_extensions, 1);
        assert_eq!(t.unvalidated, 1);
    }

    #[test]
    fn an_empty_result_summarizes_to_zeroes() {
        let t = summarize(&[], &known());
        assert_eq!(t.total_files, 0);
        assert_eq!(t.total_bytes, 0);
        assert!(t.by_kind.is_empty());
        assert!(t.duplicate_sha256.is_empty());
        assert_eq!(t.modified_min, None);
    }
}
