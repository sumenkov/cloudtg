#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Manager;
use cloudtg_lib::state::AppState;
use cloudtg_lib::commands;

static ICON_LOGGED: AtomicBool = AtomicBool::new(false);

fn load_app_icon() -> Option<tauri::image::Image<'static>> {
  let bytes = include_bytes!("../icons/icon.png");
  match tauri::image::Image::from_bytes(bytes) {
    Ok(img) => Some(img.to_owned()),
    Err(e) => {
      tracing::warn!("Не удалось загрузить иконку окна: {e}");
      None
    }
  }
}

fn log_icon_applied_once() {
  if !ICON_LOGGED.swap(true, Ordering::Relaxed) {
    tracing::debug!("Иконка окна установлена");
  }
}

fn apply_webview_icon(window: &tauri::WebviewWindow, icon: &tauri::image::Image<'static>) {
  if let Err(e) = window.set_icon(icon.clone()) {
    tracing::warn!("Не удалось установить иконку окна: {e}");
  } else {
    log_icon_applied_once();
  }
}

fn main() {
  let _ = dotenvy::dotenv();
  cloudtg_lib::logging::init();
  let icon_for_setup = load_app_icon();

  tauri::Builder::default()
    .manage(AppState::new())
    .plugin(tauri_plugin_clipboard_manager::init())
    .invoke_handler(tauri::generate_handler![
      commands::auth_status,
      commands::auth_start,
      commands::auth_submit_code,
      commands::auth_submit_password,
      commands::storage_get_or_create_channel,
      commands::dir_create,
      commands::dir_rename,
      commands::dir_move,
      commands::dir_delete,
      commands::dir_list_tree,
      commands::file_list,
      commands::file_pick,
      commands::file_upload,
      commands::file_move,
      commands::file_delete,
      commands::file_delete_many,
      commands::file_download,
      commands::file_open,
      commands::file_open_folder,
      commands::file_share_link,
      commands::file_share_to_chat,
      commands::tg_search_chats,
      commands::tg_recent_chats,
      commands::tg_test_message,
      commands::tg_create_channel,
      commands::tg_sync_storage,
      commands::settings_get_tg,
      commands::settings_set_tg,
      commands::settings_unlock_tg
    ])
    .setup(move |app| {
      if let Some(icon) = icon_for_setup.clone() {
        for (_, win) in app.webview_windows() {
          apply_webview_icon(&win, &icon);
        }
      }
      let state = app.state::<AppState>();
      state.spawn_init(app.handle().clone());
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
