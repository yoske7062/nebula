use crate::state::{AppState, AppStatus};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::State;

#[cfg(not(windows))]
const DEFAULT_GAME_PATH: &str = "/Applications/League of Legends.app/Contents/LoL/Game";
#[cfg(windows)]
const DEFAULT_GAME_PATH: &str = r"C:\Riot Games\League of Legends\Game";

#[cfg(not(windows))]
const GAME_BINARY: &str = "LeagueofLegends.app";
#[cfg(windows)]
const GAME_BINARY: &str = "League of Legends.exe";

/// Check if a directory is a valid League game folder by looking for the game binary.
fn is_valid_game_dir(path: &str) -> bool {
    Path::new(path).join(GAME_BINARY).exists()
}

/// canonicalize, minus the \\?\ prefix Windows adds, which would leak into the UI and mod-tools args.
fn canonical(path: &str) -> Option<String> {
    let full = Path::new(path).canonicalize().ok()?.to_string_lossy().to_string();
    Some(full.strip_prefix(r"\\?\").map(String::from).unwrap_or(full))
}

/// Try multiple path suffixes to find the Game directory.
fn resolve_game_path(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }

    let mut candidates = vec![
        format!("{}/Contents/LoL/Game", raw),
        raw.to_string(),
        format!("{}/Game", raw),
    ];
    // The file dialog returns a file on Windows (LeagueClient.exe, League of Legends.exe), so try its folder too.
    if let Some(dir) = Path::new(raw).parent().map(|p| p.to_string_lossy().to_string()) {
        candidates.push(format!("{}/Game", dir));
        candidates.push(dir);
    }

    candidates
        .iter()
        .find(|c| is_valid_game_dir(c))
        .and_then(|c| canonical(c))
}

/// Looks for League in its default location and records it in the app state.
pub fn detect_default_path(state: &Arc<Mutex<AppState>>) -> Result<Option<String>, String> {
    if is_valid_game_dir(DEFAULT_GAME_PATH) {
        let path = canonical(DEFAULT_GAME_PATH)
            .ok_or_else(|| format!("Could not resolve {}", DEFAULT_GAME_PATH))?;

        let mut app_state = state.lock().map_err(|e| e.to_string())?;
        app_state.game_path = Some(path.clone());
        app_state.status = AppStatus::GameDetected;
        return Ok(Some(path));
    }

    let mut app_state = state.lock().map_err(|e| e.to_string())?;
    app_state.status = AppStatus::GameNotFound;
    Ok(None)
}

#[tauri::command]
pub fn detect_game_path(state: State<'_, Arc<Mutex<AppState>>>) -> Result<Option<String>, String> {
    detect_default_path(state.inner())
}

#[tauri::command]
pub fn validate_game_path(path: String) -> Result<Option<String>, String> {
    Ok(resolve_game_path(&path))
}

#[tauri::command]
pub fn set_game_path(path: String, state: State<'_, Arc<Mutex<AppState>>>) -> Result<(), String> {
    let validated = resolve_game_path(&path)
        .ok_or_else(|| format!("Invalid game path: {} not found", GAME_BINARY))?;

    let mut app_state = state.lock().map_err(|e| e.to_string())?;
    app_state.game_path = Some(validated);
    app_state.status = AppStatus::GameDetected;
    Ok(())
}

#[tauri::command]
pub fn get_game_path(state: State<'_, Arc<Mutex<AppState>>>) -> Result<Option<String>, String> {
    let app_state = state.lock().map_err(|e| e.to_string())?;
    Ok(app_state.game_path.clone())
}
