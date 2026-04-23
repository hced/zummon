// src/focus.rs - Window finding (direct + heuristics + process detection)
use anyhow::Result;
use std::path::Path;
use tokio::process::Command;
use tracing::debug;
use crate::adapters::niri::NiriAdapter;
use crate::traits::Adapter;
use crate::zummon_debug;

// ============================================================================
// Process Detection (OS-aware)
// ============================================================================

#[cfg(target_os = "linux")]
pub async fn is_process_running(binary: &str) -> Result<bool> {
    let binary_name = Path::new(binary)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    if pgrep_match(&binary_name, true).await? {
        return Ok(true);
    }

    for suffix in ["-bin", "-browser", "-stable", "-beta", "-nightly", "-dev"] {
        if let Some(stripped) = binary_name.strip_suffix(suffix) {
            if pgrep_match(stripped, false).await? {
                debug!("Found process matching stripped suffix '{}' -> '{}'", binary_name, stripped);
                return Ok(true);
            }
        }
    }

    for prefix in ["bin-", "browser-", "stable-", "beta-", "nightly-", "dev-"] {
        if let Some(stripped) = binary_name.strip_prefix(prefix) {
            if pgrep_match(stripped, false).await? {
                debug!("Found process matching stripped prefix '{}' -> '{}'", binary_name, stripped);
                return Ok(true);
            }
        }
    }

    let parts: Vec<&str> = binary_name.split('-').collect();

    if let Some(first) = parts.first() {
        if *first != binary_name && pgrep_match(first, false).await? {
            debug!("Found process matching first part '{}'", first);
            return Ok(true);
        }
    }

    if let Some(last) = parts.last() {
        if *last != binary_name && pgrep_match(last, false).await? {
            debug!("Found process matching last part '{}'", last);
            return Ok(true);
        }
    }

    if parts.len() > 2 {
        for part in &parts[1..parts.len()-1] {
            if pgrep_match(part, false).await? {
                debug!("Found process matching middle part '{}'", part);
                return Ok(true);
            }
        }
    }

    if binary_name.ends_with(".AppImage") || binary_name.ends_with(".appimage") {
        let basename = binary_name
            .replace(".AppImage", "")
            .replace(".appimage", "");

        if pgrep_match(&basename, false).await? {
            debug!("Found process matching basename '{}'", basename);
            return Ok(true);
        }

        let basename_parts: Vec<&str> = basename.split('-').collect();
        if let Some(first) = basename_parts.first() {
            if *first != basename && pgrep_match(first, false).await? {
                debug!("Found process matching basename first part '{}'", first);
                return Ok(true);
            }
        }
        if let Some(last) = basename_parts.last() {
            if *last != basename && pgrep_match(last, false).await? {
                debug!("Found process matching basename last part '{}'", last);
                return Ok(true);
            }
        }
    }

    Ok(false)
}

#[cfg(target_os = "macos")]
pub async fn is_process_running(binary: &str) -> Result<bool> {
    let binary_name = Path::new(binary)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let output = Command::new("pgrep")
        .arg("-i")
        .arg(&binary_name)
        .output()
        .await?;

    if output.status.success() {
        return Ok(true);
    }

    let output = Command::new("ps")
        .arg("-eo")
        .arg("comm")
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().any(|line| line.contains(&binary_name)))
}

#[cfg(target_os = "windows")]
pub async fn is_process_running(binary: &str) -> Result<bool> {
    let binary_name = Path::new(binary)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let output = Command::new("tasklist")
        .arg("/FI")
        .arg(format!("IMAGENAME eq {}.exe", binary_name))
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(!stdout.contains("No tasks") && !stdout.contains("INFO: No tasks"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub async fn is_process_running(_binary: &str) -> Result<bool> {
    Ok(false)
}

#[cfg(target_os = "linux")]
async fn pgrep_match(pattern: &str, exact: bool) -> Result<bool> {
    let mut cmd = Command::new("pgrep");
    if exact {
        cmd.arg("-x");
    } else {
        cmd.arg("-f");
    }
    cmd.arg(pattern);

    let output = cmd.output().await?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.lines().next().is_none() {
            debug!("pgrep found match for '{}'", pattern);
            return Ok(true);
        }
    }

    Ok(false)
}

// ============================================================================
// Name Variants for Fuzzy Matching
// ============================================================================

/// Generate structural variants of a binary name for fuzzy matching
pub fn generate_variants(s: &str) -> Vec<String> {
    let mut variants = vec![s.to_string()];

    let without_ext = s
        .replace(".AppImage", "")
        .replace(".appimage", "");

    if without_ext != s {
        variants.push(without_ext.clone());
    }

    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() > 1 {
        variants.push(parts[0].to_string());
        variants.push(parts[parts.len()-1].to_string());
    }

    let ext_parts: Vec<&str> = without_ext.split('-').collect();
    if ext_parts.len() > 1 {
        let first = ext_parts[0].to_string();
        let last = ext_parts[ext_parts.len()-1].to_string();
        if first != parts[0] || ext_parts.len() != parts.len() {
            variants.push(first);
        }
        if last != parts[parts.len()-1] || ext_parts.len() != parts.len() {
            variants.push(last);
        }
    }

    let mut unique: Vec<String> = variants
        .into_iter()
        .map(|v| v.to_lowercase())
        .collect();
    unique.sort();
    unique.dedup();

    unique
}

// ============================================================================
// Jaro-Winkler Fuzzy Window Matching
// ============================================================================

/// Jaro-Winkler fuzzy matching for windows (Niri-specific)
pub async fn find_window_with_heuristics(adapter: &NiriAdapter, binary: &str) -> Result<Option<String>> {
    zummon_debug!("Applying Jaro-Winkler heuristics to find window for: {}", binary);

    let windows = adapter.get_windows_json().await?;
    let candidates: Vec<&String> = windows
        .iter()
        .filter_map(|w| w.app_id.as_ref())
        .collect();

    if candidates.is_empty() {
        return Ok(None);
    }

    let binary_name = Path::new(binary)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    let variants = generate_variants(&binary_name);
    zummon_debug!("Testing variants: {:?}", variants);

    let mut best_match = None;
    let mut best_score = 0.0;

    for candidate in &candidates {
        let cand_lower = candidate.to_lowercase();

        for variant in &variants {
            let score = jaro_winkler::jaro_winkler(variant, &cand_lower);

            if score > best_score {
                best_score = score;
                best_match = Some((*candidate, score));
            }
        }
    }

    if let Some((app_id, score)) = best_match {
        zummon_debug!("Best Jaro-Winkler match: '{}' with score {:.3}", app_id, score);

        if score >= 0.6 {
            adapter.find_window(app_id).await
        } else {
            zummon_debug!("Score too low ({:.3} < 0.6), rejecting", score);
            Ok(None)
        }
    } else {
        Ok(None)
    }
}
