// src/adapters/hyprland.rs - Hyprland window system implementation
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
pub struct HyprlandWindow {
    pub address: String,
    pub mapped: bool,
    pub hidden: bool,
    pub at: [i32; 2],
    pub size: [i32; 2],
    pub workspace: HyprlandWorkspace,
    pub floating: bool,
    pub pseudo: bool,
    pub monitor: i32,
    pub class: String,
    pub title: String,
    #[serde(rename = "initialClass")]
    pub initial_class: String,
    #[serde(rename = "initialTitle")]
    pub initial_title: String,
    pub pid: u32,
    pub xwayland: bool,
    pub pinned: bool,
    pub fullscreen: u8,
    #[serde(rename = "fullscreenClient")]
    pub fullscreen_client: u8,
    pub grouped: Vec<String>,
    pub swallowing: String,
    #[serde(rename = "focusHistoryID")]
    pub focus_history_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyprlandWorkspace {
    pub id: i32,
    pub name: String,
}

pub struct HyprlandAdapter;

impl HyprlandAdapter {
    pub async fn new() -> Result<Self> {
        if which::which("hyprctl").is_err() {
            return Err(anyhow!("hyprctl command not found in PATH"));
        }
        Ok(Self)
    }

    async fn hyprctl(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("hyprctl")
            .args(args)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("hyprctl failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub(crate) async fn get_windows_json(&self) -> Result<Vec<HyprlandWindow>> {
        let output = self.hyprctl(&["clients", "-j"]).await?;
        if output.trim().is_empty() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_str(&output)?)
    }

    async fn get_active_window(&self) -> Result<Option<HyprlandWindow>> {
        let output = self.hyprctl(&["activewindow", "-j"]).await?;
        if output.trim().is_empty() {
            return Ok(None);
        }
        Ok(serde_json::from_str(&output).ok())
    }

    fn get_window_state_category(&self, window: &HyprlandWindow) -> WindowState {
        if window.floating {
            return WindowState::Floating;
        }

        match window.fullscreen {
            1 => WindowState::Fullscreen,
            2 => WindowState::MaximizeEdges,
            _ => {
                if window.pseudo {
                    WindowState::Floating
                } else {
                    WindowState::MaximizeEdges
                }
            }
        }
    }

    #[allow(dead_code)]
    pub async fn spawn_and_discover_app_id(&mut self, cmd_str: &str) -> Result<Option<String>> {
        let pre_windows = self.get_windows_json().await?;
        let pre_addresses: Vec<String> = pre_windows.iter().map(|w| w.address.clone()).collect();

        zummon_debug!("Pre-spawn windows: {}", pre_addresses.len());

        self.spawn_command_string(cmd_str).await?;

        let timeout_ms = 5000;
        let interval_ms = 100;
        let max_attempts = timeout_ms / interval_ms;

        for attempt in 0..max_attempts {
            tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;

            let post_windows = self.get_windows_json().await?;

            for window in &post_windows {
                if !pre_addresses.contains(&window.address) {
                    zummon_debug!("New window detected after {}ms: address={}, class={}",
                           attempt * interval_ms, window.address, window.class);

                    return Ok(Some(window.class.clone()));
                }
            }
        }

        zummon_debug!("Timeout waiting for new window to appear");
        Ok(None)
    }

    pub async fn find_window_with_heuristics(&self, binary: &str) -> Result<Option<String>> {
        let windows = self.get_windows_json().await?;
        let candidates: Vec<&String> = windows.iter().map(|w| &w.class).collect();

        if candidates.is_empty() {
            return Ok(None);
        }

        let binary_name = std::path::Path::new(binary)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();

        let variants = crate::focus::generate_variants(&binary_name);
        zummon_debug!("Testing variants: {:?}", variants);

        let mut best_match = None;
        let mut best_score = 0.0;

        for candidate in candidates {
            let cand_lower = candidate.to_lowercase();
            for variant in &variants {
                let score = jaro_winkler::jaro_winkler(variant, &cand_lower);
                if score > best_score {
                    best_score = score;
                    best_match = Some((candidate.clone(), score));
                }
            }
        }

        if let Some((app_id, score)) = best_match {
            zummon_debug!("Best Jaro-Winkler match: '{}' with score {:.3}", app_id, score);
            if score >= 0.6 {
                self.find_window(&app_id).await
            } else {
                zummon_debug!("Score too low ({:.3} < 0.6), rejecting", score);
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
}

#[async_trait]
impl Adapter for HyprlandAdapter {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn find_window(&self, app_id: &str) -> Result<Option<String>> {
        let windows = self.get_windows_json().await?;
        let app_id_lower = app_id.to_lowercase();

        if tracing::enabled!(tracing::Level::DEBUG) {
            let available_ids: Vec<String> = windows
                .iter()
                .map(|w| w.class.clone())
                .collect();
            zummon_debug!("Available app_ids: {:?}", available_ids);
        }

        let matching = windows
            .iter()
            .filter(|w| w.class.to_lowercase().ends_with(&app_id_lower))
            .last();

        Ok(matching.map(|w| w.address.clone()))
    }

    async fn get_focused_window(&self) -> Result<Option<String>> {
        Ok(self.get_active_window().await?.map(|w| w.address))
    }

    async fn focus_window(&self, window_id: &str) -> Result<()> {
        self.hyprctl(&["dispatch", "focuswindow", &format!("address:{}", window_id)]).await?;
        Ok(())
    }

    async fn spawn_command_string(&mut self, cmd_str: &str) -> Result<()> {
        let hypr_cmd = format!("hyprctl dispatch exec -- {}", cmd_str);
        zummon_debug!("Executing: {}", hypr_cmd);

        let child = StdCommand::new("sh")
            .arg("-c")
            .arg(&hypr_cmd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to spawn hyprctl command")?;

        let pid = child.id();
        zummon_debug!("Spawned with PID: {}", pid);

        std::mem::forget(child);

        zummon_debug!("Spawn command executed successfully");
        Ok(())
    }

    async fn get_window_ids(&self) -> Result<Vec<String>> {
        let windows = self.get_windows_json().await?;
        Ok(windows.iter().map(|w| w.address.clone()).collect())
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
            .find(|w| w.address == window_id)
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
                    WindowState::Fullscreen => {
                        self.hyprctl(&["dispatch", "fullscreen", "1"]).await?;
                    }
                    WindowState::MaximizeEdges => {
                        self.hyprctl(&["dispatch", "fullscreen", "2"]).await?;
                    }
                    WindowState::Floating => {
                        self.hyprctl(&["dispatch", "togglefloating", &format!("address:{}", window_id)]).await?;
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
