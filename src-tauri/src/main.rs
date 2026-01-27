#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cloudtg_lib::state::AppState;
use cloudtg_lib::commands;

fn main() {
  cloudtg_lib::logging::init();

  tauri::Builder::default()
    .manage(AppState::new())
    .invoke_handler(tauri::generate_handler![
      commands::auth_status,
      commands::auth_start,
      commands::auth_submit_code,
      commands::auth_submit_password,
      commands::storage_get_or_create_channel,
      commands::dir_create,
      commands::dir_list_tree
    ])
    .setup(|app| {
      let state = app.state::<AppState>();
      state.spawn_init(app.handle().clone());
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
