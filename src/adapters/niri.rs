// src/adapters/niri.rs - Niri window system implementation
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
pub struct NiriWindow {
    pub id: u64,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub is_focused: bool,
    pub is_floating: bool,
    pub workspace_id: Option<u64>,
    #[serde(default)]
    pub layout: Option<NiriWindowLayout>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NiriWindowLayout {
    #[serde(default)]
    pub window_size: Option<[u32; 2]>,
    #[serde(default)]
    pub position: Option<[i32; 2]>,
}

pub struct NiriAdapter;

impl NiriAdapter {
    pub async fn new() -> Result<Self> {
        if which::which("niri").is_err() {
            return Err(anyhow!("niri command not found in PATH"));
        }
        Ok(Self)
    }

    async fn niri_msg(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("niri")
            .arg("msg")
            .args(args)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("niri msg failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub(crate) async fn get_windows_json(&self) -> Result<Vec<NiriWindow>> {
        let output = self.niri_msg(&["--json", "windows"]).await?;
        if output.trim().is_empty() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_str(&output)?)
    }

    async fn get_screen_dimensions(&self) -> Result<(u32, u32)> {
        let output = self.niri_msg(&["outputs"]).await?;
        for line in output.lines() {
            if line.contains("Logical size:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    let dims: Vec<&str> = parts[2].split('x').collect();
                    if dims.len() == 2 {
                        return Ok((
                            dims[0].parse().unwrap_or(1920),
                            dims[1].parse().unwrap_or(1080),
                        ));
                    }
                }
            }
        }
        Ok((1920, 1080))
    }

    fn get_window_state_category(&self, window: &NiriWindow, screen_w: u32, screen_h: u32) -> WindowState {
        if window.is_floating {
            return WindowState::Floating;
        }

        if let Some(layout) = &window.layout {
            if let Some(size) = layout.window_size {
                let win_w = size[0];
                let win_h = size[1];

                let w_diff = (screen_w as i32 - win_w as i32).abs();
                let h_diff = (screen_h as i32 - win_h as i32).abs();

                if w_diff <= 8 && h_diff <= 8 {
                    return WindowState::Fullscreen;
                }

                let h_tolerance = (screen_h as f32 * 0.05) as i32;
                if w_diff <= 100 && h_diff <= h_tolerance {
                    return WindowState::MaximizeEdges;
                }
            }
        }

        WindowState::Floating
    }

    #[allow(dead_code)]
    pub async fn find_window_with_heuristics(&self, binary: &str) -> Result<Option<String>> {
        use crate::focus::find_window_with_heuristics;
        find_window_with_heuristics(self, binary).await
    }

    pub async fn spawn_and_discover_app_id(&mut self, cmd_str: &str) -> Result<Option<String>> {
        let pre_windows = self.get_windows_json().await?;
        let pre_window_ids: Vec<String> = pre_windows.iter().map(|w| w.id.to_string()).collect();

        zummon_debug!("Pre-spawn windows: {}", pre_window_ids.len());

        self.spawn_command_string(cmd_str).await?;

        let timeout_ms = 5000;
        let interval_ms = 100;
        let max_attempts = timeout_ms / interval_ms;

        for attempt in 0..max_attempts {
            tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;

            let post_windows = self.get_windows_json().await?;

            for window in &post_windows {
                if !pre_window_ids.contains(&window.id.to_string()) {
                    zummon_debug!("New window detected after {}ms: id={}, app_id={:?}",
                           attempt * interval_ms, window.id, window.app_id);

                    if let Some(app_id) = &window.app_id {
                        return Ok(Some(app_id.clone()));
                    }
                }
            }
        }

        zummon_debug!("Timeout waiting for new window to appear");
        Ok(None)
    }
}

#[async_trait]
impl Adapter for NiriAdapter {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn find_window(&self, app_id: &str) -> Result<Option<String>> {
        let windows = self.get_windows_json().await?;
        let app_id_lower = app_id.to_lowercase();

        if tracing::enabled!(tracing::Level::DEBUG) {
            let available_ids: Vec<String> = windows
                .iter()
                .filter_map(|w| w.app_id.clone())
                .collect();
            zummon_debug!("Available app_ids: {:?}", available_ids);
        }

        let matching = windows
            .iter()
            .filter(|w| {
                w.app_id
                    .as_ref()
                    .map(|id| id.to_lowercase().ends_with(&app_id_lower))
                    .unwrap_or(false)
            })
            .last();

        Ok(matching.map(|w| w.id.to_string()))
    }

    async fn get_focused_window(&self) -> Result<Option<String>> {
        let windows = self.get_windows_json().await?;
        Ok(windows
            .iter()
            .find(|w| w.is_focused)
            .map(|w| w.id.to_string()))
    }

    async fn focus_window(&self, window_id: &str) -> Result<()> {
        self.niri_msg(&["action", "focus-window", "--id", window_id]).await?;
        Ok(())
    }

    async fn spawn_command_string(&mut self, cmd_str: &str) -> Result<()> {
        let niri_cmd = format!("niri msg action spawn -- {}", cmd_str);
        zummon_debug!("Executing: {}", niri_cmd);

        let child = StdCommand::new("sh")
            .arg("-c")
            .arg(&niri_cmd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to spawn niri command")?;

        let pid = child.id();
        zummon_debug!("Spawned with PID: {}", pid);

        std::mem::forget(child);

        zummon_debug!("Spawn command executed successfully");
        Ok(())
    }

    async fn get_window_ids(&self) -> Result<Vec<String>> {
        let windows = self.get_windows_json().await?;
        Ok(windows.iter().map(|w| w.id.to_string()).collect())
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
        let windows = self.get_windows_json().await?;
        let window = windows
            .iter()
            .find(|w| w.id.to_string() == window_id)
            .context(format!("Window not found: {}", window_id))?;

        let (screen_w, screen_h) = self.get_screen_dimensions().await?;
        let current_category = self.get_window_state_category(window, screen_w, screen_h);

        if let Some(layout) = &window.layout {
            if let Some(size) = layout.window_size {
                zummon_debug!("Window {} current: category={:?}, size={}x{}, floating={}, screen={}x{}",
                       window_id, current_category, size[0], size[1], window.is_floating, screen_w, screen_h);
            }
        }

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
                    WindowState::Fullscreen => {
                        self.niri_msg(&["action", "fullscreen-window", "--id", window_id]).await?;
                    }
                    WindowState::MaximizeEdges => {
                        self.niri_msg(&["action", "maximize-window-to-edges", "--id", window_id]).await?;
                    }
                    WindowState::Floating => {
                        self.niri_msg(&["action", "move-window-to-floating", "--id", window_id]).await?;
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
