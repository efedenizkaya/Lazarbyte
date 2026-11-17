# Competitive analysis and feature backlog (owner-approved 2026-11-17)

Research report comparing Lazarbyte with open-source recovery tools and
commercial Android apps, evaluated against the project's actual state
(Faz 0-8 shipped). Findings below are the re-prioritised backlog; the
original report's P0 items that assumed Faz 3/6/7 were pending are already
implemented and are not repeated as work.

## Already shipped (no work needed)

Quick/deep scans, thumbnails, type filters, select-and-export, foreground
scans, trash browser with system-consent restore, watched-folder capture,
honest empty-state messages, filesDir + SAF export targets, explicit
refusals: no FTP/cloud (INTERNET permission is absent), no permanent
"clean/wipe" feature, no MANAGE_EXTERNAL_STORAGE.

## Approved backlog

### P0 (next)

1. **Triage summary** — a core `summarize` over the accumulated results:
   counts per format family, total and largest files, duplicate SHA-256
   values, extension-vs-content mismatches, and (for undelete results) the
   modified-time range. Surfaced as a summary card above the results list.
2. **Re-verify recovered files** — hash the recovered bytes again and compare
   against the manifest SHA-256; a per-result re-verified badge. Chain of
   custody for exported files.
3. **Cross-scan dedupe** — quick (undelete) and deep (carve) results
   accumulate into one list; rows whose SHA-256 already appeared are marked
   as duplicates instead of surfacing twice.

### P1 (after P0)

4. Result-list filters: search by name, size floor, modified-date range
   (display-level filtering in Kotlin; the decision data stays in Rust).
5. **Identify**: content-based type detection over recovered bytes to
   correct or flag wrong extensions before export (reuses the carving
   signature table).
6. **Pre-scan info**: a cheap inventory ("what is in this image?") before a
   full scan, for large image files.
7. Chain-of-custody display: source image hash, scan time, tool version on
   the results screen.
8. **Device-to-image copy** (needs a separate go): image a failing OTG card
   into private storage with bad-sector tolerance before scanning it. The
   Source/Sink layer already supports it; storage and battery costs must be
   stated in the UI.

### P2 (pool, owner's call on order)

9. ext4 `crtime` in the undelete Timestamps record.
10. User-configurable retention for captured copies (a "delete after N days"
    setting, balancing storage).
11. User-imported hash-set filtering (NSRL-style) as JSON, not embedded lists.
12. Hardening of `--sig-file` custom signatures toward the declarative
    fixed/size_field/footer model with mandatory max_size.
13. TR/EN interface strings.

### Out of scope (owner decisions standing)

- ext4 journal replay and bifragment reassembly (roadmap §7 requires showing
  these before building; both are large, low-confidence features).
- JPEG MCU-level repair (the reference tool's pipeline is C++/OpenCV: toolchain
  and license risk).
- FTP/cloud transfer (INTERNET permission is absent by rule), any permanent
  "wipe" feature (contradicts the recovery purpose), MANAGE_EXTERNAL_STORAGE
  (refused; the invisible-other-apps limit is stated in UI and README).

## Verification notes

Project names and feature lists above come from web research summaries;
before any code references them, the relevant repository's LICENSE file must
be read (unearth: claimed MIT; Reclaim, ext4fs-forensic, helix-salvager:
unverified). GPL/AGPL sources (TestDisk/PhotoRec, Sleuth Kit, OpenCV
pipelines) may be read for behaviour only — no translation, no adaptation
(AGENTS.md §2). Any new dependency goes through the license gate and
`cargo deny check`.
