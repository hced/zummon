// src/adapters/windows.rs - Windows window system implementation
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use crate::cli::WindowStateFlag;
use crate::traits::{Adapter, WindowState};
use crate::zummon_debug;
use std::process::Command as StdCommand;
use std::process::Stdio;
use std::path::Path;

pub struct WindowsAdapter;

impl WindowsAdapter {
    pub async fn new() -> Result<Self> {
        Ok(Self)
    }

    #[allow(dead_code)]
    fn get_window_state_category(&self, _window: &WindowsWindow) -> WindowState {
        WindowState::MaximizeEdges
    }

    async fn get_windows_powershell(&self) -> Result<Vec<WindowsWindow>> {
        let script = r#"
            Add-Type @"
            using System;
            using System.Runtime.InteropServices;
            using System.Text;
            public class Win32Window {
                [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
                [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
                [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr hWnd);
                [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
                [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
                public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
            }
"@
            $windows = @()
            $callback = {
                param($hwnd, $lParam)
                if ([Win32Window]::IsWindowVisible($hwnd)) {
                    $length = [Win32Window]::GetWindowTextLength($hwnd)
                    $sb = New-Object System.Text.StringBuilder($length + 1)
                    [Win32Window]::GetWindowText($hwnd, $sb, $sb.Capacity) | Out-Null
                    $title = $sb.ToString()
                    if ($title -ne "" -and $title.Length -gt 0) {
                        $pid = 0
                        [Win32Window]::GetWindowThreadProcessId($hwnd, [ref]$pid) | Out-Null
                        $proc = Get-Process -Id $pid -ErrorAction SilentlyContinue
                        $appId = if ($proc) { $proc.ProcessName } else { "" }
                        $windows += @{
                            id = $hwnd.ToString()
                            title = $title
                            app_id = $appId
                            pid = $pid
                        }
                    }
                }
                return $true
            }
            $delegate = [Win32Window+EnumWindowsProc]$callback
            [Win32Window]::EnumWindows($delegate, [IntPtr]::Zero) | Out-Null
            $windows | ConvertTo-Json -Compress
        "#;

        let output = StdCommand::new("powershell")
            .args(["-Command", script])
            .output()?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() {
            return Ok(Vec::new());
        }

        #[derive(Deserialize)]
        struct PsWindow {
            id: String,
            app_id: String,
            title: String,
            pid: u32,
        }

        let windows: Vec<PsWindow> = serde_json::from_str(&stdout)?;
        Ok(windows.into_iter().map(|w| WindowsWindow {
            id: w.id,
            app_id: w.app_id,
            title: w.title,
            pid: w.pid,
        }).collect())
    }

    pub async fn find_window_with_heuristics(&self, binary: &str) -> Result<Option<String>> {
        let windows = self.get_windows_powershell().await?;
        let candidates: Vec<&String> = windows.iter().map(|w| &w.app_id).collect();

        if candidates.is_empty() {
            return Ok(None);
        }

        let binary_name = Path::new(binary)
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

    #[allow(dead_code)]
    pub async fn spawn_and_discover_app_id(&mut self, _cmd_str: &str) -> Result<Option<String>> {
        Ok(None)
    }
}

#[async_trait]
impl Adapter for WindowsAdapter {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn find_window(&self, app_id: &str) -> Result<Option<String>> {
        let windows = self.get_windows_powershell().await?;
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
        let script = r#"
            Add-Type @"
            using System;
            using System.Runtime.InteropServices;
            public class Win32 {
                [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
                [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
            }
"@
            $hwnd = [Win32]::GetForegroundWindow()
            $hwnd.ToString()
        "#;

        let output = StdCommand::new("powershell")
            .args(["-Command", script])
            .output()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(Some(stdout.trim().to_string()))
        } else {
            Ok(None)
        }
    }

    async fn focus_window(&self, window_id: &str) -> Result<()> {
        let script = format!(
            r#"
            Add-Type @"
            using System;
            using System.Runtime.InteropServices;
            public class Win32 {{
                [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
                [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
            }}
"@
            $hwnd = [IntPtr]::new({0})
            [Win32]::ShowWindow($hwnd, 9) | Out-Null
            [Win32]::SetForegroundWindow($hwnd) | Out-Null
            "#,
            window_id
        );

        StdCommand::new("powershell")
            .args(["-Command", &script])
            .output()?;

        Ok(())
    }

    async fn spawn_command_string(&mut self, cmd_str: &str) -> Result<()> {
        zummon_debug!("Executing: {}", cmd_str);

        let child = StdCommand::new("cmd")
            .args(["/C", cmd_str])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        zummon_debug!("Spawned with PID: {}", child.id());
        std::mem::forget(child);
        Ok(())
    }

    async fn get_window_ids(&self) -> Result<Vec<String>> {
        let windows = self.get_windows_powershell().await?;
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

    async fn apply_states_to_window(&self, _window_id: &str, _states: &[WindowState]) -> Result<()> {
        Ok(())
    }

    async fn apply_window_state(&self, _pre_spawn_ids: &[String], _states: &[WindowState]) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct WindowsWindow {
    id: String,
    app_id: String,
    title: String,
    pid: u32,
}
