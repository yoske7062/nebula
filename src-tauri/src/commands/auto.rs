use crate::state::AppState;
use serde::Serialize;
use serde_json::Value;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

static ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ChampSelect {
    connected: bool,
    in_champ_select: bool,
    champion_id: u32,
    skin_num: u32,
}

const NOT_IN_SELECT: ChampSelect = ChampSelect {
    connected: false,
    in_champ_select: false,
    champion_id: 0,
    skin_num: 0,
};

#[tauri::command]
pub fn set_auto_mode(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// The client writes its lockfile next to the Game folder: name:pid:port:password:protocol.
fn read_lockfile(game_path: &str) -> Option<(u16, String)> {
    let raw = std::fs::read_to_string(Path::new(game_path).parent()?.join("lockfile")).ok()?;
    let mut parts = raw.trim().split(':').skip(2);
    let port = parts.next()?.parse().ok()?;
    let password = parts.next()?.to_string();
    Some((port, password))
}

fn parse_ux_args(line: &str) -> Option<(u16, String)> {
    let (mut port, mut token) = (None, None);
    for arg in line.split_whitespace() {
        if let Some(v) = arg.strip_prefix("--app-port=") {
            port = v.parse().ok();
        } else if let Some(v) = arg.strip_prefix("--remoting-auth-token=") {
            token = Some(v.to_string());
        }
    }
    Some((port?, token?))
}

// ponytail: the lockfile is not always where we look on every client build, so
// fall back to the UX process arguments, which always carry port and token.
fn process_credentials() -> Option<(u16, String)> {
    let out = Command::new("ps").args(["-ax", "-o", "command="]).output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.contains("LeagueClientUx"))
        .find_map(parse_ux_args)
}

// ponytail: shells out to curl instead of pulling in a TLS stack; swap for an
// in-process client if per-poll process spawns ever matter. -k is loopback-only
// (the client serves a self-signed cert) and the password goes via stdin so it
// never shows up in `ps`.
fn lcu_get(port: u16, password: &str, path: &str) -> Option<Value> {
    let mut child = Command::new("curl")
        .args(["-sk", "--max-time", "2", "-K", "-"])
        .arg(format!("https://127.0.0.1:{port}{path}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child
        .stdin
        .take()?
        .write_all(format!("user = \"riot:{password}\"\n").as_bytes())
        .ok()?;
    let out = child.wait_with_output().ok()?;
    serde_json::from_slice(&out.stdout).ok()
}

fn parse_session(session: &Value) -> ChampSelect {
    let me = session["localPlayerCellId"].as_i64();
    let player = session["myTeam"]
        .as_array()
        .and_then(|team| team.iter().find(|p| me.is_some() && p["cellId"].as_i64() == me));
    match player {
        Some(p) => ChampSelect {
            connected: true,
            in_champ_select: true,
            champion_id: p["championId"].as_u64().unwrap_or(0) as u32,
            skin_num: (p["selectedSkinId"].as_u64().unwrap_or(0) % 1000) as u32,
        },
        None => NOT_IN_SELECT,
    }
}

/// Polls the League client while auto mode is on and emits `lcu-champ-select`
/// whenever the local player's pick changes.
pub fn spawn_watcher(app: AppHandle, state: Arc<Mutex<AppState>>) {
    std::thread::spawn(move || {
        let mut last: Option<ChampSelect> = None;
        loop {
            std::thread::sleep(Duration::from_millis(1500));
            if !ENABLED.load(Ordering::Relaxed) {
                last = None;
                continue;
            }

            let game_path = state.lock().ok().and_then(|s| s.game_path.clone());
            let current = game_path
                .as_deref()
                .and_then(read_lockfile)
                .or_else(process_credentials)
                .and_then(|(port, pw)| lcu_get(port, &pw, "/lol-champ-select/v1/session"))
                .map(|v| ChampSelect { connected: true, ..parse_session(&v) })
                .unwrap_or(NOT_IN_SELECT);

            if last.as_ref() != Some(&current) {
                app.emit("lcu-champ-select", &current).ok();
                last = Some(current);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finds_local_player_pick() {
        let session = json!({
            "localPlayerCellId": 2,
            "myTeam": [
                {"cellId": 0, "championId": 11, "selectedSkinId": 11001},
                {"cellId": 2, "championId": 266, "selectedSkinId": 266007}
            ]
        });
        let cs = parse_session(&session);
        assert!(cs.in_champ_select);
        assert_eq!(cs.champion_id, 266);
        assert_eq!(cs.skin_num, 7);
    }

    #[test]
    fn error_body_is_not_champ_select() {
        let err = json!({"errorCode": "RPC_ERROR", "httpStatus": 404});
        assert!(parse_session(&err) == NOT_IN_SELECT);
    }

    #[test]
    fn reads_ux_process_args() {
        let line = "/Applications/League of Legends.app/Contents/LoL/League of Legends.app/Contents/MacOS/LeagueClientUx --riotclient-app-port=1111 --app-port=5678 --riotclient-auth-token=nope --remoting-auth-token=tok_en-1 --locale=en_US";
        assert_eq!(parse_ux_args(line), Some((5678, "tok_en-1".to_string())));
        assert_eq!(parse_ux_args("LeagueClientUx --locale=en_US"), None);
    }

    #[test]
    fn reads_port_and_password() {
        let dir = std::env::temp_dir().join("nebula_lock_test/LoL");
        std::fs::create_dir_all(dir.join("Game")).unwrap();
        std::fs::write(dir.join("lockfile"), "LeagueClient:1234:5678:secret:https").unwrap();
        let got = read_lockfile(dir.join("Game").to_str().unwrap());
        assert_eq!(got, Some((5678, "secret".to_string())));
    }
}
