use serde::de::DeserializeOwned;
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::{DndEnableArgs, DndStatus};

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> crate::Result<Dnd<R>> {
    let handle = api.register_android_plugin("com.sutauruki.studybites.dnd", "DndPlugin")?;
    Ok(Dnd(handle))
}

pub struct Dnd<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Dnd<R> {
    pub fn status(&self) -> crate::Result<DndStatus> {
        self.0.run_mobile_plugin("status", ()).map_err(Into::into)
    }

    pub fn request_access(&self) -> crate::Result<()> {
        self.0
            .run_mobile_plugin("requestAccess", ())
            .map_err(Into::into)
    }

    pub fn enable(&self, args: DndEnableArgs) -> crate::Result<DndStatus> {
        self.0.run_mobile_plugin("enable", args).map_err(Into::into)
    }

    pub fn restore(&self) -> crate::Result<DndStatus> {
        self.0.run_mobile_plugin("restore", ()).map_err(Into::into)
    }
}
