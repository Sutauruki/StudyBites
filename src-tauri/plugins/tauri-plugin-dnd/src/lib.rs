use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

pub use models::*;

#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(target_os = "android")]
mod mobile;

mod commands;
mod error;
pub mod logic;
mod models;

pub use error::{Error, Result};

#[cfg(not(target_os = "android"))]
use desktop::Dnd;
#[cfg(target_os = "android")]
use mobile::Dnd;

/// Extensions to [`tauri::App`], [`tauri::AppHandle`] and [`tauri::Window`] to access the dnd APIs.
pub trait DndExt<R: Runtime> {
    fn dnd(&self) -> &Dnd<R>;
}

impl<R: Runtime, T: Manager<R>> crate::DndExt<R> for T {
    fn dnd(&self) -> &Dnd<R> {
        self.state::<Dnd<R>>().inner()
    }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("dnd")
        .invoke_handler(tauri::generate_handler![
            commands::dnd_status,
            commands::dnd_request_access,
            commands::dnd_enable,
            commands::dnd_restore
        ])
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let dnd = mobile::init(app, api)?;
            #[cfg(not(target_os = "android"))]
            let dnd = desktop::init(app, api)?;
            app.manage(dnd);
            Ok(())
        })
        .build()
}
