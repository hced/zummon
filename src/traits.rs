// src/traits.rs - Window system abstraction layer
use anyhow::Result;
use async_trait::async_trait;
use crate::cli::WindowStateFlag;

#[derive(Debug, Clone, PartialEq)]
pub enum WindowState {
    Fullscreen,
    MaximizeEdges,
    Floating,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Platform {
    Linux(LinuxWindowSystem),
    MacOS,
    Windows,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinuxWindowSystem {
    Niri,
    Hyprland,
    Kwin,
    Mutter,
    Sway,
    Unknown,
}

impl Platform {
    pub fn detect() -> Self {
        match std::env::consts::OS {
            "linux" => Platform::Linux(LinuxWindowSystem::detect()),
            "macos" => Platform::MacOS,
            "windows" => Platform::Windows,
            _ => Platform::Unknown,
        }
    }
}

impl LinuxWindowSystem {
    pub fn detect() -> Self {
        // Niri
        if std::env::var("NIRI_SOCKET").is_ok() {
            return LinuxWindowSystem::Niri;
        }

        // Hyprland
        if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
            return LinuxWindowSystem::Hyprland;
        }

        // Sway
        if std::env::var("SWAYSOCK").is_ok() {
            return LinuxWindowSystem::Sway;
        }

        // Check XDG_CURRENT_DESKTOP for KDE/GNOME
        if let Ok(desktop) = std::env::var("XDG_CURRENT_DESKTOP") {
            let desktop_lower = desktop.to_lowercase();
            if desktop_lower.contains("kde") && std::env::var("WAYLAND_DISPLAY").is_ok() {
                return LinuxWindowSystem::Kwin;
            }
            if desktop_lower.contains("gnome") && std::env::var("WAYLAND_DISPLAY").is_ok() {
                return LinuxWindowSystem::Mutter;
            }
        }

        LinuxWindowSystem::Unknown
    }

    pub fn name(&self) -> &'static str {
        match self {
            LinuxWindowSystem::Niri => "Niri",
            LinuxWindowSystem::Hyprland => "Hyprland",
            LinuxWindowSystem::Kwin => "KWin (KDE Plasma)",
            LinuxWindowSystem::Mutter => "Mutter (GNOME)",
            LinuxWindowSystem::Sway => "Sway",
            LinuxWindowSystem::Unknown => "Unknown",
        }
    }
}

#[async_trait]
pub trait Adapter: Send + Sync {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
    async fn find_window(&self, app_id: &str) -> Result<Option<String>>;
    async fn get_focused_window(&self) -> Result<Option<String>>;
    async fn focus_window(&self, window_id: &str) -> Result<()>;
    async fn spawn_command_string(&mut self, cmd_str: &str) -> Result<()>;
    async fn get_window_ids(&self) -> Result<Vec<String>>;
    async fn validate_states(&self, states: Vec<WindowStateFlag>) -> Result<Vec<WindowState>>;
    async fn apply_states_to_window(&self, window_id: &str, states: &[WindowState]) -> Result<()>;
    async fn apply_window_state(&self, pre_spawn_ids: &[String], states: &[WindowState]) -> Result<()>;
}
