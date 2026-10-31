//! Windows patcher: LTK Manager's injection host, the same one Rose uses.
//! cslol's runoverlay no longer applies anything on current League builds, so
//! mod-tools only builds the overlay and the LTK host serves it to the game.
#![cfg_attr(not(windows), allow(dead_code))]

use super::command;
use crate::state::PatcherStatus;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};

const HOST: &str = "ltk_patcher_host.exe";
const DLL: &str = "ltk_patcher_dll.dll";
/// Hook flag 4 opts out of the DLL's base-skin check: Nebula replaces skin0, which
/// that check rejects by disabling the whole overlay (Rose sets the same flag).
const FLAGS: u32 = 4;
const LOG_INFO: u32 = 0x10;
const NOT_INSTALLED: &str = "LTK patcher not found. Install LTK Manager \
    (github.com/LeagueToolkit/ltk-manager/releases), open it once so it downloads its patcher, then apply again.";
const OUT_OF_DATE: &str = "The LTK patcher is too old for this League patch. Open LTK Manager so it updates, then apply again.";
const TOO_LATE: &str = "The match started before Nebula was ready. Apply before the game loads.";

/// LTK Manager stages every patcher DLL it downloads in
/// %APPDATA%\dev.leaguetoolkit.manager\patcher\<digest>\, and copies its host next
/// to it once it has run a session. The newest DLL wins.
fn find_patcher() -> Result<(PathBuf, PathBuf), String> {
    let appdata = std::env::var_os("APPDATA").ok_or(NOT_INSTALLED)?;
    let staged = Path::new(&appdata).join("dev.leaguetoolkit.manager").join("patcher");
    let dir = fs::read_dir(&staged)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|d| d.join(DLL).is_file())
        .max_by_key(|d| fs::metadata(d.join(DLL)).and_then(|m| m.modified()).ok())
        .ok_or(NOT_INSTALLED)?;
    let installed = std::env::var_os("ProgramFiles")
        .map(|p| Path::new(&p).join("LTK Manager").join(HOST))
        .unwrap_or_default();
    let host = [dir.join(HOST), installed]
        .into_iter()
        .find(|p| p.is_file())
        .ok_or(NOT_INSTALLED)?;
    Ok((host, dir.join(DLL)))
}

/// Starts the host and has it scan for the game; it serves `overlay` from then on.
/// The host injects the DLL next to it, so both run from Nebula's own folder.
pub fn spawn_host(work_dir: &Path, overlay: &Path) -> Result<Child, String> {
    let (host, dll) = find_patcher()?;
    let dir = work_dir.join("ltk");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (src, name) in [(&host, HOST), (&dll, DLL)] {
        fs::copy(src, dir.join(name)).map_err(|e| format!("Could not copy {}: {}", src.display(), e))?;
    }

    let mut child = command(dir.join(HOST))
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start LTK patcher: {}", e))?;

    let prefix = format!("{}\\", overlay.to_string_lossy().trim_end_matches(['\\', '/']));
    let setup = format!("config loglevel {LOG_INFO}\nconfig flags {FLAGS}\nconfig prefix {prefix}\nstart scan\n");
    child
        .stdin
        .as_mut()
        .ok_or("LTK patcher has no stdin")?
        .write_all(setup.as_bytes())
        .map_err(|e| format!("Could not configure LTK patcher: {}", e))?;
    Ok(child)
}

/// Maps one host event line ("status <ts> <state> <msg>", "error <ts> <msg>",
/// "dll <ts> <pid> <tid> <level> <msg>") to the matching patcher status.
pub fn status(line: &str) -> Option<PatcherStatus> {
    let mut words = line.split_whitespace();
    match words.next()? {
        "status" => {
            words.next(); // timestamp
            let state = words.next()?;
            let message = words.collect::<Vec<_>>().join(" ");
            match state {
                "injecting" => Some(PatcherStatus::WaitingForGame),
                "injected" => Some(PatcherStatus::FoundGame),
                "waiting" => Some(PatcherStatus::InGame),
                "exited" => Some(PatcherStatus::GameExited),
                "failed" => Some(PatcherStatus::Error(format!("Patcher failed: {message}"))),
                _ => None,
            }
        }
        "error" => {
            words.next(); // timestamp
            Some(PatcherStatus::Error(format!("Patcher error: {}", words.collect::<Vec<_>>().join(" "))))
        }
        "dll" if line.contains("end of life reached") => Some(PatcherStatus::Error(OUT_OF_DATE.into())),
        "dll" if line.contains("joined too late") => Some(PatcherStatus::Error(TOO_LATE.into())),
        "dll" if line.contains("disabling overlay") => Some(PatcherStatus::Error(format!(
            "The patcher rejected the skin: {}",
            line.split_once(" ERROR ").map_or(line, |(_, m)| m)
        ))),
        _ => None,
    }
}

/// magic + version (4), RSA signature (256), checksum (8)
const WAD_HEADER: usize = 268;

/// Since 16.19 the game refuses overlay WADs that carry mkoverlay's own signature
/// and a zero checksum ("WadFile mount failed"). Copy the game's header onto each
/// overlay WAD, as LTK Manager and Rose do. Returns how many were restored.
pub fn restore_wad_headers(overlay: &Path, game: &Path) -> usize {
    let mut restored = 0;
    for wad in wad_files(overlay) {
        let Some(original) = wad
            .strip_prefix(overlay)
            .ok()
            .and_then(|rel| read_header(&game.join(rel)))
        else {
            continue;
        };
        if &original[..2] != b"RW" {
            continue;
        }
        let Ok(mut file) = fs::OpenOptions::new().read(true).write(true).open(&wad) else {
            continue;
        };
        // Only a WAD of the same version shares the header layout.
        let mut magic = [0u8; 4];
        if file.read_exact(&mut magic).is_err() || magic != original[..4] {
            continue;
        }
        if file.write_all(&original[4..]).is_ok() {
            restored += 1;
        }
    }
    restored
}

fn read_header(path: &Path) -> Option<[u8; WAD_HEADER]> {
    let mut header = [0u8; WAD_HEADER];
    fs::File::open(path).ok()?.read_exact(&mut header).ok()?;
    Some(header)
}

fn wad_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(wad_files(&path));
        } else if path.to_string_lossy().ends_with(".wad.client") {
            found.push(path);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_host_events() {
        assert_eq!(status("status 0.1 injecting scanning for game"), Some(PatcherStatus::WaitingForGame));
        assert_eq!(status("status 9.0 waiting overlay active"), Some(PatcherStatus::InGame));
        assert_eq!(status("status 60.0 failed DLL never attached"), Some(PatcherStatus::Error("Patcher failed: DLL never attached".into())));
        assert_eq!(status("ok 0.2 config prefix set"), None);
        assert_eq!(status("dll 1.0 12 34 ERROR end of life reached, please update: 1700000000"), Some(PatcherStatus::Error(OUT_OF_DATE.into())));
        assert_eq!(status("dll 1.0 12 34 INFO redirected wad: DATA/FINAL/Champions/Ahri.wad.client"), None);
    }

    #[test]
    fn copies_game_header_onto_overlay_wads() {
        let root = std::env::temp_dir().join("nebula_wad_header_test");
        let _ = fs::remove_dir_all(&root);
        let (game, overlay) = (root.join("Game"), root.join("overlay"));
        let rel = Path::new("DATA/FINAL/Champions/Ahri.wad.client");
        fs::create_dir_all(game.join(rel).parent().unwrap()).unwrap();
        fs::create_dir_all(overlay.join(rel).parent().unwrap()).unwrap();

        let mut game_wad = b"RW\x03\x04".to_vec();
        game_wad.extend([0xAB; WAD_HEADER - 4]);
        game_wad.extend(b"game body");
        fs::write(game.join(rel), &game_wad).unwrap();

        let mut mod_wad = b"RW\x03\x04".to_vec();
        mod_wad.extend([0; WAD_HEADER - 4]);
        mod_wad.extend(b"mod body");
        fs::write(overlay.join(rel), &mod_wad).unwrap();

        assert_eq!(restore_wad_headers(&overlay, &game), 1);
        let out = fs::read(overlay.join(rel)).unwrap();
        assert_eq!(out[..WAD_HEADER], game_wad[..WAD_HEADER]);
        assert_eq!(&out[WAD_HEADER..], b"mod body");
    }
}
