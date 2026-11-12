use crate::{DndEnableArgs, DndExt, DndMode, DndStatus, Result};
use tauri::{command, AppHandle, Runtime};

#[command]
pub(crate) async fn dnd_status<R: Runtime>(app: AppHandle<R>) -> Result<DndStatus> {
    app.dnd().status()
}

#[command]
pub(crate) async fn dnd_request_access<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    app.dnd().request_access()
}

#[command]
pub(crate) async fn dnd_enable<R: Runtime>(
    app: AppHandle<R>,
    mode: DndMode,
    restore_after_minutes: u32,
) -> Result<DndStatus> {
    app.dnd().enable(DndEnableArgs {
        mode,
        restore_after_minutes,
    })
}

#[command]
pub(crate) async fn dnd_restore<R: Runtime>(app: AppHandle<R>) -> Result<DndStatus> {
    app.dnd().restore()
}
