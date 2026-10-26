# Keeping Nebula working after a League update

Nebula's patcher (`mod-tools`, from cslol-manager) edits the running game process.
It breaks when a League patch changes the game binary or the mod file formats.
Upstream (Mouadzz/zushi, MIT) fixes these quickly; Nebula was forked at upstream
commit `9dd1b2e` (v0.1.12) and has ported everything up to v0.1.15 (`8e2b63f`).

## How a skin gets applied

1. `mkoverlay` builds an overlay folder from the skin zips (`wad` files that replace the originals).
2. `runoverlay` waits for the game, then via `task_for_pid` (needs root) it
   - suspends the game with `task_suspend` (never `ptrace`: League calls `PT_DENY_ATTACH` and macOS kills the attacher),
   - finds the `wad_verify` call in `__text` and the `fopen` import pointer + stub,
   - redirects the `fopen` import stub to a shellcode hook that serves overlay files for wad paths,
   - resumes the game.
3. The game reads overlay files as the logged-in user, so the overlay must live in the user's home, not `/var/root`.

## Failure map (symptom, cause, fix)

| Symptom | Cause | Where it was fixed |
|---|---|---|
| Stuck on "Scanning game", log `Failed to find fopen org` | 16.21+ binaries use chained fixups: no `LC_DYLD_INFO`, `_fopen` lives in `__DATA_CONST,__got` | `macho.hpp`: look it up through the indirect symbol table (`read_import_ptrs`) |
| "In game" but no skin | overlay under `/var/root` unreadable by the game (EACCES in the fopen hook) | `lib.rs`: `adopt_user_home` / `--user-home` |
| Patcher dies with SIGSEGV on apply | `ptrace(PT_ATTACH)` against a `PT_DENY_ATTACH` game | use `task_suspend`/`task_resume`, resume on every exit path |
| Patch sometimes silently missed | fopen hook raced dyld; `Process::Base()` picked the wrong region | install hook by redirecting the import stub while suspended; pick the executable region backed by the game file |
| Custom mod crashes game (`ALE-... Missing data: 0x0`) or white textures | mods built before ~16.19: renamed shared bins, missing self-ID hash, asset paths now 64-bit hashes | `wad/repair.cpp` `repair_outdated_bins()` |
| Skin list empty or stale | skin-data fetch failed | fall back to stale cache |

## Procedure after every League patch

1. Get the game version: `defaults read "/Applications/League of Legends.app/Contents/LoL/Game/LeagueofLegends.app/Contents/Info" CFBundleVersion`.
2. Check the binary: `otool -arch arm64 -l <Game binary> | grep LC_DYLD`, `otool -arch arm64 -Iv <Game binary> | grep _fopen`.
3. Run the Mach-O lookup against the real binary (no game needed): parse it with `MachO::parse_data_arm64` and call `find_import_ptr("_fopen")` and `find_stub_refs`. Both must be non-zero. `find_wad_verify` in `patcher_macos_arm64.cpp` is the other scan that can break.
4. Look at what upstream changed: `git -C ~/Desktop/game/zushi-upstream pull && git log --oneline`.
5. Port it: `git diff <last ported commit> HEAD -- mod-tools src-tauri/src src > p.patch`, rename `Zushi`->`Nebula`, `zushi`->`nebula` inside the patch, `patch -p1 --dry-run < p.patch`, then apply.
6. Rebuild: `cmake -S mod-tools -B mod-tools/build -DCMAKE_BUILD_TYPE=Release && cmake --build mod-tools/build`, copy to `src-tauri/binaries/mod-tools-aarch64-apple-darwin`, then `npx tauri build --bundles app`.
7. If the patch exposes a new break upstream has not fixed, compare the failing scan against the new binary with `otool`/`nm` and adapt the scan in `patcher_macos_arm64.cpp`.

Last ported upstream commit: `8e2b63f` (v0.1.15, 2026-10-22).
