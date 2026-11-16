# Faz 7 implementation plan (owner-approved 2026-11-16)

Work happens in a fresh session; this file is the handoff. Follow AGENTS.md §6/§7
(checks + small commits) as everywhere else.

## Verified API facts (developer.android.com/training/data-storage/shared/media)

- Trash exists on Android 11+ (API 30). `MediaStore.createTrashRequest(resolver,
  uris, true)` returns a PendingIntent; the user approves a system dialog; files
  get `IS_TRASHED = 1` and sit as `.trashed-<ts>-<name>` in their original
  directory, purged by the system after ~30 days.
- Restoring works the same way: `createTrashRequest(resolver, uris, false)` —
  the user approves the dialog. This is exactly our use case (we did not trash
  the files ourselves).
- Querying trashed items: filter `IS_TRASHED = 1` on the media collections.
  Reading other apps' media needs `READ_MEDIA_IMAGES/VIDEO/AUDIO` (manifest +
  runtime request). No network involvement, so §3 is untouched.
- `MANAGE_MEDIA` (API 31+) skips the per-operation dialogs. NOT used for now:
  a recovery app silently moving media is a trust question; revisit only with
  the owner's explicit approval.

## Steps (one commit each)

1. **Trash browser screen**: query `IS_TRASHED = 1` across media collections
   (`MediaStore.Files` view + per-volume content URIs); list name, size,
   original relative path, and how long the system will keep the file (the
   `.trashed-` timestamp + the ~30 day purge). Restore single or many through
   `createTrashRequest(..., false)`; surface the system dialog result honestly
   (a denial is not an error to hide).
2. **Prevention / capture-before-delete**: the user picks watch folders via
   SAF `OpenDocumentTree` (persisted tree grants); a `ContentObserver` on the
   media store watches for changes and a foreground service copies affected
   files' bytes into app-private storage before/when they change. HONEST
   POSITIONING (§4, README): root-less Android cannot block a deletion; what
   this buys is a copy of the file being already safe when the deletion lands,
   plus early detection of trash transitions. Nothing more is promised, in
   the UI or the README.
3. **CI/README**: smoke test the new screen on the emulator; document the new
   permissions and the prevention limits in the README's Android section
   (GitHub-only distribution; no Play Store — see AGENTS.md §8).

## Risks

- `createTrashRequest` has device-specific bugs reported (Samsung Android 16,
  SD-card trashing). Emulator tests only here; real-device behaviour is a
  question mark to state in the README, not to paper over.
- `ContentObserver` fires after the change; the "prevention" wording above is
  the honest ceiling of what is implementable without root.
- Persisted tree grants must be re-requested after app data clears; keep the
  grant lifecycle explicit in the UI.
