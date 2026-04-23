// src/adapters/macos.rs - macOS window system implementation
use anyhow::{Result, Context, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use crate::cli::WindowStateFlag;
use crate::traits::{Adapter, WindowState};
use crate::zummon_debug;
use std::process::Command as StdCommand;
use std::process::Stdio;
use std::path::Path;
use crate::focus;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacOSWindow {
    pub id: String,
    pub app_id: String,
    pub title: String,
    pub is_focused: bool,
    pub pid: u32,
}

pub struct MacOSAdapter;

impl MacOSAdapter {
    pub async fn new() -> Result<Self> {
        let output = Command::new("which")
            .arg("osascript")
            .output()
            .await?;

        if !output.status.success() {
            return Err(anyhow!("osascript not found"));
        }
        Ok(Self)
    }

    async fn osascript(&self, script: &str) -> Result<String> {
        let output = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("osascript failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    async fn get_windows_applescript(&self) -> Result<Vec<MacOSWindow>> {
        let script = r#"
            set output to ""
            tell application "System Events"
                repeat with proc in (every process whose background only is false)
                    set procName to name of proc
                    set procPid to unix id of proc
                    set isFront to frontmost of proc
                    repeat with win in (every window of proc)
                        set winTitle to name of win
                        set winId to id of win
                        set output to output & procName & "|" & winId & "|" & winTitle & "|" & isFront & "|" & procPid & "\n"
                    end repeat
                end repeat
            end tell
            return output
        "#;

        let output = self.osascript(script).await?;
        let mut windows = Vec::new();

        for line in output.lines() {
            if line.is_empty() { continue; }
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() == 5 {
                windows.push(MacOSWindow {
                    app_id: parts[0].to_string(),
                    id: parts[1].to_string(),
                    title: parts[2].to_string(),
                    is_focused: parts[3] == "true",
                    pid: parts[4].parse().unwrap_or(0),
                });
            }
        }

        Ok(windows)
    }

    #[allow(dead_code)]
    fn get_window_state_category(&self, _window: &MacOSWindow) -> WindowState {
        WindowState::MaximizeEdges
    }

    pub async fn find_window_with_heuristics(&self, binary: &str) -> Result<Option<String>> {
        let windows = self.get_windows_applescript().await?;
        let candidates: Vec<&String> = windows.iter().map(|w| &w.app_id).collect();

        if candidates.is_empty() {
            return Ok(None);
        }

        let binary_name = Path::new(binary)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();

        let variants = focus::generate_variants(&binary_name);
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

    #[allow(dead_code)]
    pub async fn spawn_and_discover_app_id(&mut self, cmd_str: &str) -> Result<Option<String>> {
        let pre_windows = self.get_windows_applescript().await?;
        let pre_ids: Vec<String> = pre_windows.iter().map(|w| w.id.clone()).collect();

        zummon_debug!("Pre-spawn windows: {}", pre_ids.len());

        self.spawn_command_string(cmd_str).await?;

        let timeout_ms = 5000;
        let interval_ms = 100;
        let max_attempts = timeout_ms / interval_ms;

        for attempt in 0..max_attempts {
            tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;

            let post_windows = self.get_windows_applescript().await?;

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
impl Adapter for MacOSAdapter {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn find_window(&self, app_id: &str) -> Result<Option<String>> {
        let windows = self.get_windows_applescript().await?;
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
        let windows = self.get_windows_applescript().await?;
        Ok(windows.iter().find(|w| w.is_focused).map(|w| w.id.clone()))
    }

    async fn focus_window(&self, window_id: &str) -> Result<()> {
        let script = format!(
            r#"
            tell application "System Events"
                repeat with proc in (every process)
                    repeat with win in (every window of proc)
                        if id of win is {} then
                            set frontmost of proc to true
                            tell proc to set index of win to 1
                            return
                        end if
                    end repeat
                end repeat
            end tell
            "#,
            window_id
        );
        self.osascript(&script).await?;
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
        let windows = self.get_windows_applescript().await?;
        Ok(windows.iter().map(|w| w.id.clone()).collect())
    }

    async fn validate_states(&self, states: Vec<WindowStateFlag>) -> Result<Vec<WindowState>> {
        let supported = vec!["fullscreen", "maximize-edges"];
        Ok(states
            .iter()
            .filter_map(|s| {
                let abstract_state = s.to_abstract_state();
                if supported.contains(&abstract_state) {
                    match abstract_state {
                        "fullscreen" => Some(WindowState::Fullscreen),
                        "maximize-edges" => Some(WindowState::MaximizeEdges),
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .collect())
    }

    async fn apply_states_to_window(&self, window_id: &str, states: &[WindowState]) -> Result<()> {
        let windows = self.get_windows_applescript().await?;
        let _window = windows
            .iter()
            .find(|w| w.id == window_id)
            .context(format!("Window not found: {}", window_id))?;

        for state in states {
            match state {
                WindowState::Fullscreen | WindowState::MaximizeEdges => {
                    let script = format!(
                        r#"
                        tell application "System Events"
                            repeat with proc in (every process)
                                repeat with win in (every window of proc)
                                    if id of win is {} then
                                        tell proc to set zoomed of win to not (zoomed of win)
                                    end if
                                end repeat
                            end repeat
                        end tell
                        "#,
                        window_id
                    );
                    self.osascript(&script).await?;
                }
                _ => {}
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
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
