// src/adapters/kwin.rs - KDE/KWin window system implementation
use anyhow::{Result, Context, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use crate::cli::WindowStateFlag;
use crate::traits::{Adapter, WindowState};
use crate::zummon_debug;
use std::process::Command as StdCommand;
use std::process::Stdio;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KdeWindow {
    pub id: String,
    pub app_id: String,
    pub title: String,
    pub is_focused: bool,
    pub is_floating: bool,
    pub is_fullscreen: bool,
    pub workspace: i32,
    pub pid: u32,
}

pub struct KwinAdapter;

impl KwinAdapter {
    pub async fn new() -> Result<Self> {
        // Check for KWin D-Bus service
        let output = Command::new("qdbus")
            .args(["org.kde.KWin", "/KWin"])
            .output()
            .await?;

        if !output.status.success() {
            return Err(anyhow!("KWin D-Bus service not available"));
        }
        Ok(Self)
    }

    async fn kwin_script(&self, script: &str) -> Result<String> {
        let output = Command::new("qdbus")
            .args(["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.loadScript"])
            .arg(script)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("KWin script failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    async fn get_windows_js(&self) -> Result<Vec<KdeWindow>> {
        let script = r#"
            const clients = workspace.clientList();
            JSON.stringify(clients.map(c => ({
                id: String(c.windowId),
                app_id: c.resourceClass || c.resourceName || '',
                title: c.caption || '',
                is_focused: c === workspace.activeClient,
                is_floating: c.floating || false,
                is_fullscreen: c.fullScreen || false,
                workspace: c.desktop,
                pid: c.pid
            })))
        "#.replace('\n', " ").replace("  ", " ");

        let output = self.kwin_script(&script).await?;
        if output.trim().is_empty() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_str(&output)?)
    }

    fn get_window_state_category(&self, window: &KdeWindow) -> WindowState {
        if window.is_floating {
            return WindowState::Floating;
        }

        if window.is_fullscreen {
            WindowState::Fullscreen
        } else {
            WindowState::MaximizeEdges
        }
    }

    #[allow(dead_code)]
    pub async fn spawn_and_discover_app_id(&mut self, cmd_str: &str) -> Result<Option<String>> {
        let pre_windows = self.get_windows_js().await?;
        let pre_ids: Vec<String> = pre_windows.iter().map(|w| w.id.clone()).collect();

        zummon_debug!("Pre-spawn windows: {}", pre_ids.len());

        self.spawn_command_string(cmd_str).await?;

        let timeout_ms = 5000;
        let interval_ms = 100;
        let max_attempts = timeout_ms / interval_ms;

        for attempt in 0..max_attempts {
            tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;

            let post_windows = self.get_windows_js().await?;

            for window in &post_windows {
                if !pre_ids.contains(&window.id) {
                    zummon_debug!("New window detected after {}ms: id={}, app_id={}",
                           attempt * interval_ms, window.id, window.app_id);

                    return Ok(Some(window.app_id.clone()));
                }
            }
        }

        zummon_debug!("Timeout waiting for new window to appear");
        Ok(None)
    }
}

#[async_trait]
impl Adapter for KwinAdapter {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn find_window(&self, app_id: &str) -> Result<Option<String>> {
        let windows = self.get_windows_js().await?;
        let app_id_lower = app_id.to_lowercase();

        if tracing::enabled!(tracing::Level::DEBUG) {
            let available_ids: Vec<String> = windows.iter().map(|w| w.app_id.clone()).collect();
            zummon_debug!("Available app_ids: {:?}", available_ids);
        }

        let matching = windows
            .iter()
            .filter(|w| w.app_id.to_lowercase().ends_with(&app_id_lower))
            .last();

        Ok(matching.map(|w| w.id.clone()))
    }

    async fn get_focused_window(&self) -> Result<Option<String>> {
        let windows = self.get_windows_js().await?;
        Ok(windows.iter().find(|w| w.is_focused).map(|w| w.id.clone()))
    }

    async fn focus_window(&self, window_id: &str) -> Result<()> {
        let script = format!(
            "const c = workspace.clientList().find(c => String(c.windowId) === '{}'); if (c) workspace.activeClient = c;",
            window_id
        );
        self.kwin_script(&script).await?;
        Ok(())
    }

    async fn spawn_command_string(&mut self, cmd_str: &str) -> Result<()> {
        zummon_debug!("Executing: {}", cmd_str);

        let child = StdCommand::new("sh")
            .arg("-c")
            .arg(cmd_str)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to spawn command")?;

        let pid = child.id();
        zummon_debug!("Spawned with PID: {}", pid);

        std::mem::forget(child);
        Ok(())
    }

    async fn get_window_ids(&self) -> Result<Vec<String>> {
        let windows = self.get_windows_js().await?;
        Ok(windows.iter().map(|w| w.id.clone()).collect())
    }

    async fn validate_states(&self, states: Vec<WindowStateFlag>) -> Result<Vec<WindowState>> {
        let supported = vec!["fullscreen", "maximize-edges", "floating"];
        Ok(states
            .iter()
            .filter_map(|s| {
                let abstract_state = s.to_abstract_state();
                if supported.contains(&abstract_state) {
                    match abstract_state {
                        "fullscreen" => Some(WindowState::Fullscreen),
                        "maximize-edges" => Some(WindowState::MaximizeEdges),
                        "floating" => Some(WindowState::Floating),
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .collect())
    }

    async fn apply_states_to_window(&self, window_id: &str, states: &[WindowState]) -> Result<()> {
        let windows = self.get_windows_js().await?;
        let window = windows
            .iter()
            .find(|w| w.id == window_id)
            .context(format!("Window not found: {}", window_id))?;

        let current_category = self.get_window_state_category(window);

        for state in states {
            let should_apply = match state {
                WindowState::Fullscreen => current_category != WindowState::Fullscreen,
                WindowState::MaximizeEdges => {
                    current_category != WindowState::MaximizeEdges
                        && current_category != WindowState::Fullscreen
                }
                WindowState::Floating => current_category != WindowState::Floating,
            };

            zummon_debug!("State {:?}: should_apply={}", state, should_apply);

            if should_apply {
                match state {
                    WindowState::Fullscreen | WindowState::MaximizeEdges => {
                        let script = format!(
                            "const c = workspace.clientList().find(c => String(c.windowId) === '{}'); if (c) c.fullScreen = !c.fullScreen;",
                            window_id
                        );
                        self.kwin_script(&script).await?;
                    }
                    WindowState::Floating => {
                        let script = format!(
                            "const c = workspace.clientList().find(c => String(c.windowId) === '{}'); if (c) c.floating = !c.floating;",
                            window_id
                        );
                        self.kwin_script(&script).await?;
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
        }
        Ok(())
    }

    async fn apply_window_state(&self, pre_spawn_ids: &[String], states: &[WindowState]) -> Result<()> {
        let timeout_ms = 3000;
        let interval_ms = 100;
        let max_attempts = timeout_ms / interval_ms;

        for _ in 0..max_attempts {
            tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;

            let post_ids = self.get_window_ids().await?;
            let new_window = post_ids
                .iter()
                .find(|id| !pre_spawn_ids.contains(id))
                .cloned();

            if let Some(window_id) = new_window {
                self.apply_states_to_window(&window_id, states).await?;
                return Ok(());
            }
        }

        tracing::warn!("[zummon {}] Timed out waiting for new window after {}ms", env!("CARGO_PKG_VERSION"), timeout_ms);
        Ok(())
    }
}
