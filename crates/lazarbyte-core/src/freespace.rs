//! Free-space maps shared by the filesystem undelete paths.
//!
//! FAT/exFAT, ext and NTFS all report the volume's unallocated regions the same
//! way, so the type and the run-coalescing rule live here rather than in any
//! one of them: the mobile profile keeps FAT/exFAT and ext without also
//! building the NTFS reader.

/// The unallocated regions of a volume, as absolute byte ranges.
pub struct FreeSpace {
    pub ranges: Vec<(u64, u64)>,
    pub free_bytes: u64,
    pub volume_bytes: u64,
}

impl FreeSpace {
    pub fn fraction(&self) -> f64 {
        if self.volume_bytes == 0 {
            0.0
        } else {
            self.free_bytes as f64 / self.volume_bytes as f64
        }
    }
}

/// Turn a run of free cluster numbers into coalesced byte ranges.
///
/// Runs separated by less than `merge_gap` are joined: reading a little
/// allocated data costs less than the seek and the per-range overhead of
/// avoiding it, and it keeps the range count sane on a fragmented volume.
pub(crate) fn ranges_from_free_clusters(
    free: impl Iterator<Item = (u64, u64)>,
    base: u64,
    cluster: u64,
    volume_end: u64,
    merge_gap: u64,
) -> (Vec<(u64, u64)>, u64) {
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    let mut free_bytes = 0u64;
    for (first, count) in free {
        let start = base.saturating_add(first.saturating_mul(cluster));
        let end = start
            .saturating_add(count.saturating_mul(cluster))
            .min(volume_end);
        if end <= start {
            continue;
        }
        free_bytes += end - start;
        match ranges.last_mut() {
            Some(last) if start.saturating_sub(last.1) <= merge_gap => last.1 = end,
            _ => ranges.push((start, end)),
        }
    }
    (ranges, free_bytes)
}
