//! Local bridge between the plugin running inside the League client and Nebula.
//! The plugin lists skins, picks one and Nebula downloads and applies it; the
//! last pick per champion is remembered so locking in the champion is enough.

use crate::commands::{patcher, skins};
use crate::state::{AppState, PatcherStatus};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use tiny_http::{Header, Method, Request, Response, Server};

const PORT: u16 = 47831;
const REPO_RAW: &str = "https://raw.githubusercontent.com/Alban1911/LeagueSkins/main/";
const SKIN_IDS_URL: &str =
    "https://raw.githubusercontent.com/Alban1911/LeagueSkins/main/resources/en/skin_ids.json";
const TREE_URL: &str =
    "https://api.github.com/repos/Alban1911/LeagueSkins/git/trees/main?recursive=1";
const SKINS_JSON_URL: &str =
    "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1/skins.json";
/// Where the client-side plugin lives; the daemon drops its access token here.
#[cfg(not(windows))]
const PLUGIN_DIR: &str = "/Users/Shared/nebula/plugins/nebula";
// ponytail: no Pengu installer on Windows yet, so nothing creates this folder and
// the in-client picker stays off; the Nebula window covers picking skins.
#[cfg(windows)]
const PLUGIN_DIR: &str = r"C:\Users\Public\nebula\plugins\nebula";
const CACHE_MAX_AGE_SECS: u64 = 12 * 3600;

#[derive(Serialize, Deserialize, Clone)]
struct ChromaInfo {
    parent: u32,
    colors: Vec<String>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Catalog {
    fetched_at: u64,
    /// skin id -> display name (champion base skins included)
    names: HashMap<u32, String>,
    /// skin id -> path inside the LeagueSkins repo
    repo: HashMap<u32, String>,
    chromas: HashMap<u32, ChromaInfo>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn data_dir(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_data_dir().expect("failed to get app data dir");
    fs::create_dir_all(&dir).ok();
    dir
}

fn get_json(url: &str) -> Result<Value, String> {
    let resp = ureq::get(url).call().map_err(|e| format!("{url}: {e}"))?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(64 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{url}: {e}"))
}

fn parse_repo_tree(tree: &Value) -> HashMap<u32, String> {
    let mut repo = HashMap::new();
    for entry in tree["tree"].as_array().into_iter().flatten() {
        let (Some(path), Some("blob")) = (entry["path"].as_str(), entry["type"].as_str()) else {
            continue;
        };
        if !path.starts_with("skins/") {
            continue;
        }
        let file = path.rsplit('/').next().unwrap_or("");
        let stem = file.strip_suffix(".fantome").or_else(|| file.strip_suffix(".zip"));
        if let Some(id) = stem.and_then(|s| s.parse::<u32>().ok()) {
            repo.insert(id, path.to_string());
        }
    }
    repo
}

fn parse_chromas(skins_json: &Value) -> HashMap<u32, ChromaInfo> {
    let mut chromas = HashMap::new();
    for skin in skins_json.as_object().into_iter().flat_map(|o| o.values()) {
        let Some(parent) = skin["id"].as_u64() else { continue };
        for c in skin["chromas"].as_array().into_iter().flatten() {
            if let Some(id) = c["id"].as_u64() {
                let colors = c["colors"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                chromas.insert(id as u32, ChromaInfo { parent: parent as u32, colors });
            }
        }
    }
    chromas
}

fn fetch_catalog() -> Result<Catalog, String> {
    let names = get_json(SKIN_IDS_URL)?
        .as_object()
        .ok_or("skin_ids.json is not an object")?
        .iter()
        .filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_string())))
        .collect();
    let repo = parse_repo_tree(&get_json(TREE_URL)?);
    // Chroma grouping is cosmetic, so a failure here must not block applying skins.
    let chromas = get_json(SKINS_JSON_URL)
        .map(|v| parse_chromas(&v))
        .unwrap_or_default();
    Ok(Catalog { fetched_at: now(), names, repo, chromas })
}

static CATALOG: Mutex<Option<Arc<Catalog>>> = Mutex::new(None);

fn is_fresh(c: &Catalog) -> bool {
    now().saturating_sub(c.fetched_at) < CACHE_MAX_AGE_SECS
}

pub fn catalog(app: &AppHandle) -> Result<Arc<Catalog>, String> {
    let mut slot = CATALOG.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = slot.as_ref().filter(|c| is_fresh(c)) {
        return Ok(c.clone());
    }

    let path = data_dir(app).join("catalog.json");
    let disk: Option<Catalog> = fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    if let Some(d) = disk.as_ref().filter(|d| is_fresh(d)) {
        let c = Arc::new(d.clone());
        *slot = Some(c.clone());
        return Ok(c);
    }

    match fetch_catalog() {
        Ok(fresh) => {
            if let Ok(bytes) = serde_json::to_vec(&fresh) {
                fs::write(&path, bytes).ok();
            }
            let c = Arc::new(fresh);
            *slot = Some(c.clone());
            Ok(c)
        }
        // Offline or rate limited: an old catalog still beats none.
        Err(e) => disk
            .map(Arc::new)
            .or_else(|| slot.clone())
            .ok_or(e),
    }
}

fn download_url(repo_path: &str) -> String {
    let encoded: Vec<String> = repo_path.split('/').map(percent_encode).collect();
    format!("{REPO_RAW}{}", encoded.join("/"))
}

fn percent_encode(seg: &str) -> String {
    seg.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ---- remembered picks -------------------------------------------------------

static SELECTION_LOCK: Mutex<()> = Mutex::new(());
static LAST_APPLIED: Mutex<Option<u32>> = Mutex::new(None);

fn selections_path(app: &AppHandle) -> PathBuf {
    data_dir(app).join("selections.json")
}

fn read_selections(app: &AppHandle) -> HashMap<u32, u32> {
    fs::read(selections_path(app))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn remember(app: &AppHandle, champion: u32, skin: u32) {
    let _guard = SELECTION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = read_selections(app);
    all.insert(champion, skin);
    if let Ok(bytes) = serde_json::to_vec(&all) {
        fs::write(selections_path(app), bytes).ok();
    }
}

// ---- operations ---------------------------------------------------------------

fn champion_name(cat: &Catalog, champion: u32) -> String {
    cat.names
        .get(&(champion * 1000))
        .cloned()
        .unwrap_or_else(|| format!("Champion {champion}"))
}

/// Skins of one champion for the picker: real skins grouped with their chromas.
fn build_skin_list(cat: &Catalog, champion: u32, selected: Option<u32>) -> Value {
    let ids: Vec<u32> = cat
        .names
        .keys()
        .copied()
        .filter(|id| id / 1000 == champion && id % 1000 != 0)
        // Chromas of the default skin are not applicable skins.
        .filter(|id| cat.chromas.get(id).map_or(true, |c| c.parent % 1000 != 0))
        .collect();

    let entry = |id: u32| {
        json!({
            "id": id,
            "num": id % 1000,
            "name": cat.names[&id],
            "available": cat.repo.contains_key(&id),
        })
    };

    let mut groups: BTreeMap<u32, (Value, Vec<Value>)> = BTreeMap::new();
    let mut chroma_ids = Vec::new();
    for &id in &ids {
        if cat.chromas.contains_key(&id) {
            chroma_ids.push(id);
        } else {
            groups.insert(id, (entry(id), Vec::new()));
        }
    }
    chroma_ids.sort_unstable();
    for id in chroma_ids {
        let info = &cat.chromas[&id];
        match groups.get_mut(&info.parent) {
            Some((_, chromas)) => {
                let mut c = entry(id);
                c["colors"] = json!(info.colors);
                chromas.push(c);
            }
            // Parent missing from the catalog: show the chroma on its own.
            None => {
                groups.insert(id, (entry(id), Vec::new()));
            }
        }
    }

    let skins: Vec<Value> = groups
        .into_values()
        .map(|(mut base, chromas)| {
            base["chromas"] = json!(chromas);
            base
        })
        .collect();

    json!({
        "champion": { "key": champion, "name": champion_name(cat, champion) },
        "selected": selected,
        "skins": skins,
    })
}

pub fn skins_for(app: &AppHandle, champion: u32) -> Result<Value, String> {
    let cat = catalog(app)?;
    let selected = read_selections(app).get(&champion).copied();
    Ok(build_skin_list(&cat, champion, selected))
}

fn patcher_is_busy(state: &Arc<Mutex<AppState>>) -> bool {
    state
        .lock()
        .map(|s| !matches!(s.patcher_status, PatcherStatus::Idle | PatcherStatus::Error(_)))
        .unwrap_or(false)
}

/// Downloads the skin if needed, applies it and remembers it for its champion.
pub fn apply_skin_id(
    app: &AppHandle,
    state: &Arc<Mutex<AppState>>,
    skin: u32,
) -> Result<(), String> {
    let cat = catalog(app)?;
    let champion = skin / 1000;
    let champ_name = champion_name(&cat, champion);
    let skin_name = cat.names.get(&skin).ok_or("Unknown skin")?;
    let repo_path = cat.repo.get(&skin).ok_or("This skin is not available")?;

    let zip = skins::skin_zip_path(app, &champ_name, skin_name);
    let zip_path = if zip.exists() {
        zip.to_string_lossy().into_owned()
    } else {
        skins::fetch_skin(app, &download_url(repo_path), &champ_name, skin_name)?
    };

    remember(app, champion, skin);
    *LAST_APPLIED.lock().unwrap_or_else(|e| e.into_inner()) = Some(skin);
    patcher::start_apply(app.clone(), vec![zip_path], state.clone())?;
    Ok(())
}

/// Called when the local player locks a champion: re-applies the remembered
/// skin unless that exact skin is already loaded and waiting for the game.
pub fn auto_apply(app: &AppHandle, state: &Arc<Mutex<AppState>>, champion: u32) {
    let Some(skin) = read_selections(app).get(&champion).copied() else {
        return;
    };
    let already = *LAST_APPLIED.lock().unwrap_or_else(|e| e.into_inner()) == Some(skin);
    if already && patcher_is_busy(state) {
        return;
    }
    if let Err(e) = apply_skin_id(app, state, skin) {
        eprintln!("[nebula] auto apply of {skin} failed: {e}");
    }
}

// ---- http server -----------------------------------------------------------------

fn random_token() -> String {
    let mut buf = [0u8; 16];
    getrandom::getrandom(&mut buf).expect("failed to get random bytes");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

fn reply(req: Request, status: u16, body: Value) {
    let response = Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(header("Content-Type", "application/json"))
        .with_header(header("Access-Control-Allow-Origin", "*"))
        .with_header(header("Access-Control-Allow-Headers", "X-Nebula-Token, Content-Type"))
        .with_header(header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"))
        .with_header(header("Access-Control-Allow-Private-Network", "true"));
    req.respond(response).ok();
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.to_string())
}

fn route(
    app: &AppHandle,
    state: &Arc<Mutex<AppState>>,
    method: &Method,
    path: &str,
    query: &str,
    body: &str,
) -> Result<Value, (u16, String)> {
    let bad = |m: &str| (400, m.to_string());
    match (method, path) {
        (Method::Get, "/skins") => {
            let champion = query_param(query, "champion")
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| bad("champion is required"))?;
            skins_for(app, champion).map_err(|e| (502, e))
        }
        (Method::Get, "/status") => {
            let status = state.lock().map(|s| s.patcher_status.clone()).unwrap_or_default();
            Ok(json!({ "patcher": status, "applied": *LAST_APPLIED.lock().unwrap_or_else(|e| e.into_inner()) }))
        }
        (Method::Post, "/apply") => {
            let skin = serde_json::from_str::<Value>(body)
                .ok()
                .and_then(|v| v["skin"].as_u64())
                .ok_or_else(|| bad("skin is required"))?;
            apply_skin_id(app, state, skin as u32).map_err(|e| (502, e))?;
            Ok(json!({ "ok": true }))
        }
        (Method::Post, "/stop") => {
            if let Ok(mut s) = state.lock() {
                if let Some(mut pipe) = s.patcher_stdin.take() {
                    use std::io::Write;
                    pipe.write_all(b"\n").ok();
                }
            }
            *LAST_APPLIED.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(json!({ "ok": true }))
        }
        _ => Err((404, "not found".to_string())),
    }
}

type Handler = Arc<dyn Fn(&Method, &str, &str, &str) -> Result<Value, (u16, String)> + Send + Sync>;

fn handle(token: &str, handler: &Handler, mut req: Request) {
    let method = req.method().clone();
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));

    if method == Method::Options {
        return reply(req, 204, json!(null));
    }
    if path == "/health" {
        return reply(req, 200, json!({ "ok": true }));
    }
    let authorized = req
        .headers()
        .iter()
        .any(|h| h.field.equiv("X-Nebula-Token") && h.value.as_str() == token);
    if !authorized {
        return reply(req, 401, json!({ "error": "unauthorized" }));
    }

    let mut body = String::new();
    req.as_reader().take(64 * 1024).read_to_string(&mut body).ok();

    match handler(&method, path, query, &body) {
        Ok(value) => reply(req, 200, value),
        Err((code, message)) => reply(req, code, json!({ "error": message })),
    }
}

fn serve(server: Server, token: String, handler: Handler) {
    for req in server.incoming_requests() {
        let (token, handler) = (token.clone(), handler.clone());
        std::thread::spawn(move || handle(&token, &handler, req));
    }
}

/// The install script creates token.js; reading it keeps plugin and daemon in sync
/// no matter which one starts first. Falls back to a fresh token if it is missing.
fn parse_token_js(text: &str) -> Option<String> {
    text.split('"').nth(1).filter(|t| t.len() >= 16).map(String::from)
}

fn load_token() -> String {
    let file = std::path::Path::new(PLUGIN_DIR).join("token.js");
    if let Some(token) = fs::read_to_string(&file).ok().as_deref().and_then(parse_token_js) {
        return token;
    }
    let token = random_token();
    if file.parent().is_some_and(|d| d.is_dir()) && fs::write(&file, format!("export default \"{token}\";\n")).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).ok();
        }
    }
    token
}

pub fn spawn_server(app: AppHandle, state: Arc<Mutex<AppState>>) {
    let token = load_token();

    // Warm the catalog so the first skin list is instant.
    let warm = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = catalog(&warm) {
            eprintln!("[nebula] catalog not ready: {e}");
        }
    });

    let handler: Handler = Arc::new(move |method, path, query, body| route(&app, &state, method, path, query, body));
    std::thread::spawn(move || match Server::http(("127.0.0.1", PORT)) {
        Ok(server) => serve(server, token, handler),
        Err(e) => eprintln!("[nebula] bridge could not bind port {PORT}: {e}"),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_fixture() -> Catalog {
        let names = [
            (266000, "Aatrox"),
            (266001, "Justicar Aatrox"),
            (266007, "Blood Moon Aatrox"),
            (266008, "Blood Moon Aatrox (Gold)"),
            (266010, "Steel Aatrox (Gold)"), // chroma of the default skin
            (267001, "Other champion skin"),
        ]
        .into_iter()
        .map(|(id, n)| (id, n.to_string()))
        .collect();
        let repo = [266001, 266007].into_iter().map(|id| (id, format!("skins/266/{id}/{id}.zip"))).collect();
        let chromas = [
            (266008, ChromaInfo { parent: 266007, colors: vec!["#ff0".into()] }),
            (266010, ChromaInfo { parent: 266000, colors: vec![] }),
        ]
        .into_iter()
        .collect();
        Catalog { fetched_at: now(), names, repo, chromas }
    }

    #[test]
    fn percent_encoding_keeps_unreserved_and_escapes_the_rest() {
        assert_eq!(percent_encode("266007.zip"), "266007.zip");
        assert_eq!(percent_encode("K/DA Kai'Sa"), "K%2FDA%20Kai%27Sa");
        assert_eq!(
            download_url("skins/266/266007/266007.zip"),
            "https://raw.githubusercontent.com/Alban1911/LeagueSkins/main/skins/266/266007/266007.zip"
        );
    }

    #[test]
    fn parses_repo_tree_by_skin_id() {
        let tree = json!({"tree": [
            {"path": "skins/266/266007/266007.zip", "type": "blob"},
            {"path": "skins/222/222020/222025/222025.fantome", "type": "blob"},
            {"path": "skins/266/readme.md", "type": "blob"},
            {"path": "skins/266", "type": "tree"},
            {"path": "resources/en/skin_ids.json", "type": "blob"}
        ]});
        let repo = parse_repo_tree(&tree);
        assert_eq!(repo.len(), 2);
        assert_eq!(repo[&222025], "skins/222/222020/222025/222025.fantome");
    }

    #[test]
    fn parses_chroma_parents() {
        let skins = json!({"266007": {"id": 266007, "chromas": [{"id": 266008, "colors": ["#ff0"]}]}});
        let chromas = parse_chromas(&skins);
        assert_eq!(chromas[&266008].parent, 266007);
        assert_eq!(chromas[&266008].colors, vec!["#ff0".to_string()]);
    }

    #[test]
    fn token_is_read_from_the_plugin_module() {
        assert_eq!(parse_token_js("export default \"0123456789abcdef\";\n"), Some("0123456789abcdef".into()));
        assert_eq!(parse_token_js("export default \"short\";"), None);
        assert_eq!(parse_token_js(""), None);
    }

    #[test]
    fn query_params_are_read() {
        assert_eq!(query_param("champion=266&x=1", "champion"), Some("266".into()));
        assert_eq!(query_param("x=1", "champion"), None);
    }

    #[test]
    fn groups_chromas_under_their_skin_and_drops_default_chromas() {
        let list = build_skin_list(&catalog_fixture(), 266, Some(266007));
        assert_eq!(list["champion"]["name"], "Aatrox");
        assert_eq!(list["selected"], 266007);

        let skins = list["skins"].as_array().unwrap();
        let ids: Vec<u64> = skins.iter().map(|s| s["id"].as_u64().unwrap()).collect();
        assert_eq!(ids, vec![266001, 266007]); // no default-skin chroma, no other champion

        let blood_moon = &skins[1];
        assert_eq!(blood_moon["available"], true);
        assert_eq!(blood_moon["chromas"][0]["id"], 266008);
        assert_eq!(blood_moon["chromas"][0]["available"], false);
        assert_eq!(skins[0]["chromas"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn server_enforces_token_and_sends_cors_headers() {
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let handler: Handler = Arc::new(|method, path, query, body| match (method, path) {
            (Method::Get, "/echo") => Ok(json!({ "query": query, "body": body })),
            (Method::Post, "/echo") => Ok(json!({ "query": query, "body": body })),
            _ => Err((404, "not found".to_string())),
        });
        std::thread::spawn(move || serve(server, "tok".into(), handler));
        let base = format!("http://127.0.0.1:{port}");

        // health needs no token
        assert_eq!(ureq::get(&format!("{base}/health")).call().unwrap().status(), 200);

        // everything else does
        match ureq::get(&format!("{base}/echo")).call() {
            Err(ureq::Error::Status(401, _)) => {}
            other => panic!("expected 401, got {other:?}"),
        }
        match ureq::get(&format!("{base}/echo")).set("X-Nebula-Token", "wrong").call() {
            Err(ureq::Error::Status(401, _)) => {}
            other => panic!("expected 401, got {other:?}"),
        }

        let ok = ureq::post(&format!("{base}/echo?a=1")).set("X-Nebula-Token", "tok").send_string("hi").unwrap();
        assert_eq!(ok.header("Access-Control-Allow-Origin"), Some("*"));
        let body: Value = serde_json::from_str(&ok.into_string().unwrap()).unwrap();
        assert_eq!(body["query"], "a=1");
        assert_eq!(body["body"], "hi");

        // browser preflight is answered without a token
        let pre = ureq::request("OPTIONS", &format!("{base}/apply")).call().unwrap();
        assert_eq!(pre.status(), 204);
        assert!(pre.header("Access-Control-Allow-Headers").unwrap().contains("X-Nebula-Token"));

        match ureq::get(&format!("{base}/nope")).set("X-Nebula-Token", "tok").call() {
            Err(ureq::Error::Status(404, _)) => {}
            other => panic!("expected 404, got {other:?}"),
        }
    }

    /// Hits GitHub and CommunityDragon: `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_catalog_lists_ahri_skins() {
        let cat = fetch_catalog().expect("catalog should download");
        println!("names={} repo={} chromas={}", cat.names.len(), cat.repo.len(), cat.chromas.len());
        let list = build_skin_list(&cat, 103, None);
        let skins = list["skins"].as_array().unwrap();
        for s in skins.iter().take(6) {
            println!(
                "{} {} available={} chromas={}",
                s["id"], s["name"], s["available"], s["chromas"].as_array().unwrap().len()
            );
        }
        assert_eq!(list["champion"]["name"], "Ahri");
        assert!(skins.len() > 5, "Ahri should have several skins");
        assert!(skins.iter().any(|s| s["available"] == true));
        assert!(!cat.repo.is_empty() && !cat.chromas.is_empty());
    }
}
