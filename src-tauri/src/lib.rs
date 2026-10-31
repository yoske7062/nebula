mod commands;
#[cfg(target_os = "macos")]
mod macos;
mod state;

use state::AppState;
use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WindowEvent};

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        window.show().ok();
        window.set_focus().ok();
    }
}

/// Nebula lives in the menu bar: no Dock icon, no window until asked for.
fn setup_background_mode(app: &mut tauri::App) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    let open = MenuItem::with_id(app, "open", "Open Nebula", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Nebula", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("Nebula")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // mod-tools needs root for task_for_pid on macOS.
    // NEBULA_DEV=1 skips the elevation so the app can be exercised without a password;
    // everything except the final task_for_pid patch step works that way.
    // Windows needs none of this: the patcher hooks the game as the same user.
    #[cfg(target_os = "macos")]
    {
        if !macos::running_as_root() && std::env::var_os("NEBULA_DEV").is_none() {
            macos::relaunch_as_admin();
            return;
        }

        eprintln!(
            "[nebula] Running as {}",
            if macos::running_as_root() { "root (euid=0)" } else { "a regular user (NEBULA_DEV)" }
        );

        macos::adopt_user_home();
    }

    let builder = tauri::Builder::default();
    // A second launch on Windows would start a second patcher; focus the running one instead.
    #[cfg(windows)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| show_main_window(app)));

    builder
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(Arc::new(Mutex::new(AppState::default())))
        .on_window_event(|window, event| {
            // Closing the window hides it; the app keeps running in the menu bar.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                window.hide().ok();
            }
        })
        .setup(|app| {
            setup_background_mode(app)?;
            let state = app.state::<Arc<Mutex<AppState>>>().inner().clone();
            // Without a game path there is nothing to patch, so ask for one.
            if !matches!(commands::game_path::detect_default_path(&state), Ok(Some(_))) {
                show_main_window(app.handle());
            }
            commands::bridge::spawn_server(app.handle().clone(), state.clone());
            commands::auto::spawn_watcher(app.handle().clone(), state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::game_path::detect_game_path,
            commands::game_path::validate_game_path,
            commands::game_path::set_game_path,
            commands::game_path::get_game_path,
            commands::patcher::apply_skin,
            commands::patcher::apply_skins,
            commands::patcher::stop_patcher,
            commands::patcher::get_patcher_status,
            commands::patcher::get_work_dir_size,
            commands::patcher::clear_work_dir,
            commands::skins::download_skin,
            commands::skins::download_multiple_skins,
            commands::skins::list_downloaded_skins,
            commands::skins::delete_downloaded_skin,
            commands::skins::get_skins_dir_size,
            commands::skins::clear_all_skins,
            commands::customs::import_custom,
            commands::customs::list_customs,
            commands::customs::remove_custom,
            commands::customs::clear_all_customs,
            commands::customs::get_customs_dir_size,
            commands::open_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
