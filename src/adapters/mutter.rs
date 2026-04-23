// src/adapters/mutter.rs - GNOME/Mutter window system implementation
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use crate::cli::WindowStateFlag;
use crate::traits::{Adapter, WindowState};
use crate::zummon_debug;
use std::process::Command as StdCommand;
use std::process::Stdio;

pub struct MutterAdapter;

impl MutterAdapter {
    pub async fn new() -> Result<Self> {
        Ok(Self)
    }

    #[allow(dead_code)]
    pub async fn spawn_and_discover_app_id(&mut self, _cmd_str: &str) -> Result<Option<String>> {
        Ok(None)
    }
}

#[async_trait]
impl Adapter for MutterAdapter {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn find_window(&self, _app_id: &str) -> Result<Option<String>> {
        Err(anyhow!(
            "GNOME/Mutter does not expose a public window management API.\n\
             Window focus/state management requires a bundled GNOME Shell extension.\n\
             Consider using a different window system or launching without focus features."
        ))
    }

    async fn get_focused_window(&self) -> Result<Option<String>> {
        Ok(None)
    }

    async fn focus_window(&self, _window_id: &str) -> Result<()> {
        Err(anyhow!("GNOME/Mutter window focusing is not supported without an extension."))
    }

    async fn spawn_command_string(&mut self, cmd_str: &str) -> Result<()> {
        zummon_debug!("Executing: {}", cmd_str);

        let child = StdCommand::new("sh")
            .arg("-c")
            .arg(cmd_str)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        zummon_debug!("Spawned with PID: {}", child.id());
        std::mem::forget(child);
        Ok(())
    }

    async fn get_window_ids(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn validate_states(&self, _states: Vec<WindowStateFlag>) -> Result<Vec<WindowState>> {
        Ok(Vec::new())
    }

    async fn apply_states_to_window(&self, _window_id: &str, _states: &[WindowState]) -> Result<()> {
        Err(anyhow!("GNOME/Mutter window state management is not supported without an extension."))
    }

    async fn apply_window_state(&self, _pre_spawn_ids: &[String], _states: &[WindowState]) -> Result<()> {
        Ok(())
    }
}
