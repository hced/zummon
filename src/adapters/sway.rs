// src/adapters/sway.rs - Sway window system implementation
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
pub struct SwayNode {
    pub id: u64,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub node_type: String,
    pub app_id: Option<String>,
    pub window_properties: Option<SwayWindowProperties>,
    pub focused: bool,
    pub fullscreen_mode: Option<u32>,
    pub floating: Option<String>,
    pub pid: Option<u32>,
    pub rect: SwayRect,
    pub nodes: Vec<SwayNode>,
    pub floating_nodes: Vec<SwayNode>,
    pub focus: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwayWindowProperties {
    pub class: Option<String>,
    pub instance: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwayRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub struct SwayAdapter;

impl SwayAdapter {
    pub async fn new() -> Result<Self> {
        if which::which("swaymsg").is_err() {
            return Err(anyhow!("swaymsg command not found in PATH"));
        }
        Ok(Self)
    }

    async fn swaymsg(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("swaymsg")
            .args(args)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("swaymsg failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    async fn get_tree(&self) -> Result<SwayNode> {
        let output = self.swaymsg(&["-t", "get_tree"]).await?;
        Ok(serde_json::from_str(&output)?)
    }

    fn find_windows(&self, node: &SwayNode, windows: &mut Vec<SwayNode>) {
        if node.node_type == "con" || node.node_type == "floating_con" {
            if node.app_id.is_some() || node.window_properties.is_some() {
                windows.push(node.clone());
            }
        }

        for child in &node.nodes {
            self.find_windows(child, windows);
        }
        for child in &node.floating_nodes {
            self.find_windows(child, windows);
        }
    }

    async fn get_all_windows(&self) -> Result<Vec<SwayNode>> {
        let tree = self.get_tree().await?;
        let mut windows = Vec::new();
        self.find_windows(&tree, &mut windows);
        Ok(windows)
    }

    fn get_window_app_id(&self, window: &SwayNode) -> Option<String> {
        window.app_id.clone().or_else(|| {
            window.window_properties.as_ref().and_then(|wp| {
                wp.class.clone().or_else(|| wp.instance.clone())
            })
        })
    }

    fn get_window_id(&self, window: &SwayNode) -> String {
        format!("{}", window.id)
    }

    fn get_window_state_category(&self, window: &SwayNode) -> WindowState {
        let is_floating = window.floating.as_ref().map(|f| f != "none").unwrap_or(false);

        if is_floating {
            return WindowState::Floating;
        }

        match window.fullscreen_mode {
            Some(1) => WindowState::Fullscreen,
            _ => WindowState::MaximizeEdges,
        }
    }

    #[allow(dead_code)]
    pub async fn spawn_and_discover_app_id(&mut self, cmd_str: &str) -> Result<Option<String>> {
        let pre_windows = self.get_all_windows().await?;
        let pre_ids: Vec<String> = pre_windows.iter().map(|w| self.get_window_id(w)).collect();

        zummon_debug!("Pre-spawn windows: {}", pre_ids.len());

        self.spawn_command_string(cmd_str).await?;

        let timeout_ms = 5000;
        let interval_ms = 100;
        let max_attempts = timeout_ms / interval_ms;

        for attempt in 0..max_attempts {
            tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;

            let post_windows = self.get_all_windows().await?;

            for window in &post_windows {
                let window_id = self.get_window_id(window);
                if !pre_ids.contains(&window_id) {
                    zummon_debug!("New window detected after {}ms: id={}, app_id={:?}",
                           attempt * interval_ms, window_id, self.get_window_app_id(window));

                    return Ok(self.get_window_app_id(window));
                }
            }
        }

        zummon_debug!("Timeout waiting for new window to appear");
        Ok(None)
    }

    pub async fn find_window_with_heuristics(&self, binary: &str) -> Result<Option<String>> {
        let windows = self.get_all_windows().await?;
        let candidates: Vec<String> = windows.iter().filter_map(|w| self.get_window_app_id(w)).collect();

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

        for candidate in &candidates {
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
impl Adapter for SwayAdapter {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn find_window(&self, app_id: &str) -> Result<Option<String>> {
        let windows = self.get_all_windows().await?;
        let app_id_lower = app_id.to_lowercase();

        if tracing::enabled!(tracing::Level::DEBUG) {
            let available_ids: Vec<Option<String>> = windows
                .iter()
                .map(|w| self.get_window_app_id(w))
                .collect();
            zummon_debug!("Available app_ids: {:?}", available_ids);
        }

        let matching = windows
            .iter()
            .filter(|w| {
                self.get_window_app_id(w)
                    .map(|id| id.to_lowercase().ends_with(&app_id_lower))
                    .unwrap_or(false)
            })
            .last();

        Ok(matching.map(|w| self.get_window_id(w)))
    }

    async fn get_focused_window(&self) -> Result<Option<String>> {
        let windows = self.get_all_windows().await?;
        Ok(windows
            .iter()
            .find(|w| w.focused)
            .map(|w| self.get_window_id(w)))
    }

    async fn focus_window(&self, window_id: &str) -> Result<()> {
        let cmd = format!("[con_id={}] focus", window_id);
        self.swaymsg(&[&cmd]).await?;
        Ok(())
    }

    async fn spawn_command_string(&mut self, cmd_str: &str) -> Result<()> {
        let sway_cmd = format!("exec {}", cmd_str);
        zummon_debug!("Executing: swaymsg '{}'", sway_cmd);

        let child = StdCommand::new("sh")
            .arg("-c")
            .arg(format!("swaymsg '{}'", sway_cmd))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to spawn sway command")?;

        let pid = child.id();
        zummon_debug!("Spawned with PID: {}", pid);

        std::mem::forget(child);

        zummon_debug!("Spawn command executed successfully");
        Ok(())
    }

    async fn get_window_ids(&self) -> Result<Vec<String>> {
        let windows = self.get_all_windows().await?;
        Ok(windows.iter().map(|w| self.get_window_id(w)).collect())
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
        let windows = self.get_all_windows().await?;
        let window = windows
            .iter()
            .find(|w| self.get_window_id(w) == window_id)
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
                        let cmd = format!("[con_id={}] fullscreen toggle", window_id);
                        self.swaymsg(&[&cmd]).await?;
                    }
                    WindowState::Floating => {
                        let cmd = format!("[con_id={}] floating toggle", window_id);
                        self.swaymsg(&[&cmd]).await?;
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
