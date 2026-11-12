use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::{DndEnableArgs, DndMode, DndStatus};

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> crate::Result<Dnd<R>> {
    Ok(Dnd(std::marker::PhantomData))
}

pub struct Dnd<R: Runtime>(std::marker::PhantomData<fn() -> R>);

impl<R: Runtime> Dnd<R> {
    pub fn status(&self) -> crate::Result<DndStatus> {
        Ok(DndStatus {
            supported: false,
            granted: false,
            mode: DndMode::Off,
        })
    }

    pub fn request_access(&self) -> crate::Result<()> {
        Ok(())
    }

    pub fn enable(&self, _args: DndEnableArgs) -> crate::Result<DndStatus> {
        self.status()
    }

    pub fn restore(&self) -> crate::Result<DndStatus> {
        self.status()
    }
}
