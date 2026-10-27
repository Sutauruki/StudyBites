mod commands;
mod state;

use state::AppState;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = AppState::load(app.handle())?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::subjects_list,
            commands::subject_get,
            commands::subject_create,
            commands::subject_rename,
            commands::subject_set_archived,
            commands::subject_delete,
            commands::topics_list,
            commands::topic_create,
            commands::topic_update,
            commands::topic_delete,
            commands::topic_reorder,
            commands::topic_nudge,
            commands::setting_get,
            commands::setting_set,
        ])
        .run(tauri::generate_context!())
        .expect("error while running StudyBites");
}
