mod commands;
mod keys;
mod phase2;
mod state;
mod worker;

use state::AppState;
use std::sync::Arc;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::load(app.handle())?;
            app.manage(state);

            // One background worker drains the ingestion queue.
            let signal = Arc::new(worker::Signal::default());
            app.manage(signal.clone());
            worker::spawn(app.handle().clone(), signal.clone());
            // Pick up anything left queued from a previous run.
            signal.wake();
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
            // Phase 2
            phase2::material_upload,
            phase2::file_preview_text,
            phase2::materials_list,
            phase2::material_set_topic,
            phase2::material_delete,
            phase2::material_save_original,
            phase2::topic_suggest,
            phase2::scheme_parse,
            phase2::scheme_save,
            phase2::explain,
            phase2::due_counts,
            phase2::session_start,
            phase2::session_grade,
            phase2::session_end,
            phase2::mastery,
            phase2::queue_status,
            phase2::queue_retry,
            phase2::providers_list,
            phase2::provider_key_set,
            phase2::github_token_set,
            phase2::github_token_present,
            phase2::update_check,
            // Phase 4
            phase2::preview_offer,
            phase2::preview_generate,
            phase2::exam_date_set,
            phase2::exam_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running StudyBites");
}
