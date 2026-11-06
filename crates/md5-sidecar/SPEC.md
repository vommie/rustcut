# md5 sidecars: rules v1

One behavior in every app that reads or writes md5 sidecars: RustCut, RustTransferFrom and RustKnownClean (user,
2026-11-04: "alle programme die md5 sidecars lesen oder schreiben sollten komplett vereinheitlicht sein und genau
gleich agieren"). This crate is the only implementation; the apps vendor it byte-identical (rule 9).

## Rules

1. **Name.** A video's sidecar is `.<file name>.md5` in the same folder (`clip.mp4` -> `.clip.mp4.md5`,
   `.hidden.mkv` -> `..hidden.mkv.md5`). Sidecars exist for videos only; which files are videos is the app's call.
   (`path_for`, `video_for`, `video_name`, `is_sidecar_name`)
2. **Written content.** Exactly the 32 lowercase hex digits of the video's MD5: no file name, no newline, no byte
   order mark. It survives renames and moves, and it is byte-identical with what PyCutRv, tresor_copy and
   RustKnownClean always wrote. (`write`)
3. **Reading.** At most 4096 bytes (a longer file is invalid). A UTF-8 byte order mark and then one leading backslash
   (md5sum's mark for an escaped name) are skipped; the first white-space-separated token must be 32 hex digits in any
   case and is lowercased; whatever follows is ignored. So the bare hash and md5sum's `<hash>  <name>` and
   `<hash> *<name>` lines all read. Anything else is invalid and treated as missing (logged at info). A sidecar that is
   not a regular file (folder, symbolic link, pipe) is invalid and never followed; one that cannot be read is invalid
   (warning). (`parse`, `lookup`)
4. **Fresh.** A valid sidecar is fresh when its modification time is not older than the video's (equal counts as
   fresh, which also holds on file systems with coarse times); only a fresh sidecar replaces hashing the video. An
   older one is stale: the video is hashed again and the sidecar rewritten. (`is_fresh`, `Lookup`)
5. **Writing.** Only after the video's last write: the app takes the video's size and time (`Stamp`) before it reads
   the bytes, and nothing is written when they changed since (`Error::VideoChanged`) or when the hash cannot be the
   video's (the zero-byte MD5 for a non-empty file or the other way round, `Error::ImpossibleHash`). The write is
   atomic: a temporary file in the same folder (`..<name>.md5.XXXXXX`), fsync, rename, fsync of the folder. A fresh
   sidecar with the same hash is left alone (`Written::Unchanged`, even in md5sum form: nothing is migrated); a
   missing, invalid or stale one is written (info); a fresh one with another hash is replaced (warning), because the new
   hash comes from the bytes. The sidecar never ends up older than its video, even when the video's time lies in the
   future (it then gets the video's time). (`write`)
6. **Moving, renaming, copying, deleting, trashing** a video does the same to its sidecar, right after the video's own
   operation succeeded:
   - move or rename (`move_along`): on the same file system the sidecar is renamed (it keeps its time, so it stays
     fresh when the video kept its time); on another one it is written in the canonical form with its old time and the
     old one removed. An invalid sidecar goes to the trash instead of along.
   - copy (`copy_along`): a valid sidecar is written next to the copy in the canonical form with its old time. It is
     fresh there only when the copy kept the video's time; otherwise the copy is hashed again later, as it should be.
     An invalid one is not copied.
   - A sidecar already at the new place belongs to no video there and goes to the trash first, unless it is fresh and
     holds the same hash; then it stays (`Carried::Kept`).
   - trash (`trash_along`): the sidecar goes to the trash too. Delete for good (`remove_along`, for example the source
     of a finished move by copy): the sidecar is deleted too.
   - A sidecar whose video is gone is an **orphan**; so is a temporary file of an interrupted write older than ten
     minutes (`LEFTOVER_AGE`). Orphans go to the trash, never away for good (`is_orphan`, `orphans_in`; the app's
     trash is handed in, so every app keeps its own trash handling).
7. A sidecar is never treated as a media file (its extension is `md5`) and never skipped as junk when its video moves:
   it goes with the video in the same step.
8. A zero-byte video may have a sidecar (`d41d8cd98f00b204e9800998ecf8427e`, `ZERO_BYTE_MD5`) but never counts as
   known (`Md5Hex::is_zero_byte`).
9. **One implementation.** This crate, vendored byte-identical into every app as `crates/md5-sidecar`. The canonical
   copy is `crates/md5-sidecar` in the RustKnownClean repository; change it only there, then run
   `scripts/sync-md5-sidecar.sh` in each app. Each app's `scripts/check-all.sh` runs `scripts/sync-md5-sidecar.sh
   --check` and fails on drift; without the canonical copy (another machine) it reports the check as skipped.
   A change of the rules raises `SPEC_VERSION` and the crate's major version.

## Test vectors

Content (Rust string notation) -> what `parse` gives. `H` = `9e107d9d372bb6826bd81d3542a419d6`. The tests in
`tests/vectors.rs` carry the same numbers.

| # | Content | Result |
|---|---|---|
| V1 | `H` | `H` |
| V2 | `"H  clip.mp4\n"` (md5sum text mode) | `H` |
| V3 | `"H *clip.mp4\n"` (md5sum binary mode) | `H` |
| V4 | `"\u{feff}H"` (byte order mark) | `H` |
| V5 | `"\\H  a\\nb.mp4\n"` (leading backslash) | `H` |
| V6 | `"\u{feff}\\H\n"` | `H` |
| V7 | `H` in uppercase | `H` (lowercased) |
| V8 | `" \tH\r\n"` | `H` |
| V9 | `"H  "` + bytes `ff fe` + `".mp4\n"` (name not UTF-8) | `H` |
| V10 | `d41d8cd98f00b204e9800998ecf8427e` | itself (valid; never "known", rule 8) |
| V11 | `H` padded with spaces to exactly 4096 bytes | `H` |
| I1 | empty | invalid: empty |
| I2 | `"\n  \r\n"` | invalid: empty |
| I3 | `"\u{feff}"` | invalid: empty |
| I4 | `"not a hash"` | invalid: not a hash |
| I5 | 31 or 33 hex digits | invalid: not a hash |
| I6 | 31 hex digits and `g` | invalid: not a hash |
| I7 | `"H*clip.mp4"` (no space) | invalid: not a hash |
| I8 | a SHA-1 (40 hex digits) | invalid: not a hash |
| I9 | `"\\\\H"` (two backslashes) | invalid: not a hash |
| I10 | `H` padded to 4097 bytes | invalid: too long |

Freshness (`is_fresh(sidecar time, video time)`):

| # | Sidecar time | Result |
|---|---|---|
| F1 | 5 s after the video's | fresh |
| F2 | equal to the video's | fresh |
| F3 | 1 ns before the video's | stale |

File behavior (`tests/files.rs`): missing, invalid, stale and fresh lookups; symbolic links and folders are invalid;
`write` creates the bare hash without a leftover, leaves a fresh equal md5sum line alone, rewrites invalid and stale
ones, replaces a fresh differing one, refuses a changed video and an impossible zero-byte hash, and keeps a sidecar
fresh for a video dated in the future; `move_along` renames with the time kept, trashes an invalid sidecar and an
orphan at the target, keeps a fresh equal one at the target and crosses file systems; `copy_along` writes the
canonical form with the old time, skips an invalid one and leaves a stale one for a copy with a new time;
`trash_along` and `remove_along` follow the video; `orphans_in` finds sidecars without their video and old write
leftovers.

## Using it

```rust
// Reading: trust only a fresh sidecar.
if let Some(hash) = md5_sidecar::lookup(&video)?.fresh() { /* known hash, no read */ }

// Writing: stamp first, hash, then write.
let stamp = md5_sidecar::Stamp::of(&video)?;
let hash = md5_sidecar::Md5Hex::from_digest(md5_of(&video)?);
md5_sidecar::write(&video, hash, stamp)?;

// Moving: the video first, then its sidecar.
std::fs::rename(&from, &to)?;
md5_sidecar::move_along(&from, &to, &mut |path| trash::delete(path).map_err(std::io::Error::other))?;
```
