//! App-level update actions. Network and disk work stay on the shared runtime.

use std::path::PathBuf;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::*;

use crate::app::{AppState, ToastLevel, runtime, update_state};
use crate::i18n::{AppLanguage, Key, tr, tr_args};
use crate::update::{self, Release, UpdateError};
use crate::update_install::{self, InstallResult};

#[derive(Clone, Debug, Default)]
pub enum UpdatePhase {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Downloaded,
    Installing,
    Manual,
    Failed(UpdateError),
}

impl UpdatePhase {
    pub fn busy(&self) -> bool {
        matches!(self, Self::Checking | Self::Downloading | Self::Installing)
    }
}

#[derive(Default)]
pub struct UpdateState {
    pub phase: UpdatePhase,
    pub release: Option<Release>,
    pub downloaded: Option<PathBuf>,
}

impl UpdateState {
    pub fn status_text(&self, lang: AppLanguage) -> String {
        match &self.phase {
            UpdatePhase::Idle => tr(lang, Key::UpdateIdle).to_string(),
            UpdatePhase::Checking => tr(lang, Key::UpdateChecking).to_string(),
            UpdatePhase::UpToDate => tr(lang, Key::UpdateCurrent).to_string(),
            UpdatePhase::Available => tr_args(
                lang,
                Key::UpdateAvailable,
                &[self.release.as_ref().map_or("", |release| release.version.as_str())],
            ),
            UpdatePhase::Downloading => tr(lang, Key::UpdateDownloading).to_string(),
            UpdatePhase::Downloaded => tr(lang, Key::UpdateDownloaded).to_string(),
            UpdatePhase::Installing => tr(lang, Key::UpdateInstalling).to_string(),
            UpdatePhase::Manual => tr(lang, Key::UpdateManual).to_string(),
            UpdatePhase::Failed(error) => tr(lang, error_key(error)).to_string(),
        }
    }
}

fn error_key(error: &UpdateError) -> Key {
    match error {
        UpdateError::Network => Key::UpdateNetworkFailed,
        UpdateError::NoRelease => Key::UpdateNoRelease,
        UpdateError::DownloadNetwork => Key::UpdateDownloadFailed,
        UpdateError::Manifest => Key::UpdateInvalidManifest,
        UpdateError::Platform => Key::UpdateNoPackage,
        UpdateError::Disk => Key::UpdateDiskFailed,
        UpdateError::Size | UpdateError::Checksum => Key::UpdateVerifyFailed,
        UpdateError::Install => Key::UpdateInstallFailed,
    }
}

impl AppState {
    pub fn toggle_updates(&mut self, cx: &mut Context<Self>) {
        if self.updates.phase.busy() {
            return;
        }
        let previous = self.config.updates_enabled;
        self.config.updates_enabled = !previous;
        if !self.persist_config(cx) {
            self.config.updates_enabled = previous;
        } else if !self.config.updates_enabled {
            self.updates = UpdateState::default();
        }
        cx.notify();
    }

    pub fn check_updates(&mut self, cx: &mut Context<Self>) {
        if !self.config.updates_enabled || self.updates.phase.busy() {
            return;
        }
        self.updates.phase = UpdatePhase::Checking;
        self.updates.release = None;
        self.updates.downloaded = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = runtime()
                .spawn(async { update::check().await })
                .await
                .unwrap_or(Err(UpdateError::Network));
            update_state(&this, cx, |state, cx| {
                if !state.config.updates_enabled {
                    return;
                }
                match result {
                    Ok(Some(release)) => {
                        state.updates.release = Some(release);
                        state.updates.phase = UpdatePhase::Available;
                    }
                    Ok(None) => state.updates.phase = UpdatePhase::UpToDate,
                    Err(error) => {
                        state.toast(ToastLevel::Error, tr(state.language(), error_key(&error)));
                        state.updates.phase = UpdatePhase::Failed(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn download_update(&mut self, cx: &mut Context<Self>) {
        if !self.config.updates_enabled || self.updates.phase.busy() {
            return;
        }
        let Some(release) = self.updates.release.clone() else {
            return;
        };
        self.updates.phase = UpdatePhase::Downloading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = runtime()
                .spawn(async move { update::download(&release).await })
                .await
                .unwrap_or(Err(UpdateError::Disk));
            update_state(&this, cx, |state, cx| {
                if !state.config.updates_enabled {
                    return;
                }
                match result {
                    Ok(path) => {
                        state.updates.downloaded = Some(path);
                        state.updates.phase = UpdatePhase::Downloaded;
                    }
                    Err(error) => {
                        state.toast(ToastLevel::Error, tr(state.language(), error_key(&error)));
                        state.updates.phase = UpdatePhase::Failed(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn confirm_update_install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.config.updates_enabled || !matches!(self.updates.phase, UpdatePhase::Downloaded) {
            return;
        }
        let app = cx.entity();
        let lang = self.language();
        let version = self
            .updates
            .release
            .as_ref()
            .map_or("", |release| release.version.as_str());
        let title = tr_args(lang, Key::UpdateInstallTitle, &[version]);
        let description = if cfg!(target_os = "macos") {
            tr(lang, Key::UpdateMacManual)
        } else {
            tr(lang, Key::UpdateInstallConfirm)
        };
        window.open_alert_dialog(cx, move |alert, _, _| {
            let app = app.clone();
            alert
                .title(title.clone())
                .description(description)
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(tr(lang, Key::UpdateInstall))
                        .cancel_text(tr(lang, Key::Cancel))
                        .show_cancel(true),
                )
                .on_ok(move |_, _, cx| {
                    app.update(cx, |state, cx| state.install_update(cx));
                    true
                })
        });
    }

    fn install_update(&mut self, cx: &mut Context<Self>) {
        if !self.config.updates_enabled || !matches!(self.updates.phase, UpdatePhase::Downloaded) {
            return;
        }
        let (Some(release), Some(path)) = (self.updates.release.clone(), self.updates.downloaded.clone()) else {
            return;
        };
        self.updates.phase = UpdatePhase::Installing;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = runtime()
                .spawn_blocking(move || update_install::start_install(&release, &path))
                .await
                .unwrap_or(Err(UpdateError::Install));
            update_state(&this, cx, |state, cx| {
                match result {
                    Ok(InstallResult::QuitApp) => cx.quit(),
                    Ok(InstallResult::Manual) => state.updates.phase = UpdatePhase::Manual,
                    Err(error) => {
                        state.toast(ToastLevel::Error, tr(state.language(), error_key(&error)));
                        state.updates.phase = UpdatePhase::Failed(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}
