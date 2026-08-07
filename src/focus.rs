// src/focus.rs - Window finding (direct + heuristics + process detection)
use crate::adapters::niri::NiriAdapter;
use crate::traits::Adapter;
use crate::zummon_debug;
use anyhow::Result;
use std::path::Path;
use sysinfo::System;

// ============================================================================
// Process Detection (cross-platform via sysinfo)
// ============================================================================
/// Find a running process matching `binary` using heuristic name matching.
/// Uses sysinfo for cross-platform process enumeration (no external tools needed).
pub fn find_process(binary: &str) -> Result<Option<sysinfo::Pid>> {
    let binary_name = Path::new(binary)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mut system = System::new_all();
    system.refresh_all();
    for process in system.processes().values() {
        let proc_name = process.name().to_string_lossy();
        // Exact match
        if proc_name == binary_name {
            return Ok(Some(process.pid()));
        }
        // Try stripped suffixes
        for suffix in ["-bin", "-browser", "-stable", "-beta", "-nightly", "-dev"] {
            if let Some(stripped) = binary_name.strip_suffix(suffix)
                && proc_name == stripped
            {
                return Ok(Some(process.pid()));
            }
        }
        // Try stripped prefixes
        for prefix in ["bin-", "browser-", "stable-", "beta-", "nightly-", "dev-"] {
            if let Some(stripped) = binary_name.strip_prefix(prefix)
                && proc_name == stripped
            {
                return Ok(Some(process.pid()));
            }
        }
        // Try hyphen parts
        let parts: Vec<&str> = binary_name.split('-').collect();
        for part in &parts {
            if proc_name == *part {
                return Ok(Some(process.pid()));
            }
        }
        // AppImage handling
        if binary_name.ends_with(".AppImage") || binary_name.ends_with(".appimage") {
            let basename = binary_name
                .replace(".AppImage", "")
                .replace(".appimage", "");
            if proc_name.starts_with(&basename) {
                return Ok(Some(process.pid()));
            }
            let basename_parts: Vec<&str> = basename.split('-').collect();
            for part in &basename_parts {
                if proc_name == *part {
                    return Ok(Some(process.pid()));
                }
            }
        }
    }
    Ok(None)
}

/// Check if a binary is currently running using heuristic name matching.
pub fn is_process_running(binary: &str) -> Result<bool> {
    Ok(find_process(binary)?.is_some())
}

/// Terminate a running process: SIGTERM on Unix (graceful), forced kill on
/// Windows (the only mechanism available there).
pub fn terminate_process(binary: &str) -> Result<()> {
    if let Some(pid) = find_process(binary)? {
        let mut system = System::new_all();
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        if let Some(process) = system.process(pid) {
            #[cfg(unix)]
            let killed = process
                .kill_with(sysinfo::Signal::Term)
                .unwrap_or_else(|| process.kill());
            #[cfg(not(unix))]
            let killed = process.kill();
            if killed {
                zummon_debug!("Terminated process for '{}' (pid {})", binary, pid);
            } else {
                anyhow::bail!("Failed to terminate process for '{}' (pid {})", binary, pid);
            }
        }
    } else {
        zummon_debug!(
            "terminate_process: no running process found for '{}'",
            binary
        );
    }
    Ok(())
}

// ============================================================================
// Name Variants for Fuzzy Matching
// ============================================================================
/// Generate structural variants of a binary name for fuzzy matching
pub fn generate_variants(s: &str) -> Vec<String> {
    let mut variants = vec![s.to_string()];
    let without_ext = s.replace(".AppImage", "").replace(".appimage", "");
    if without_ext != s {
        variants.push(without_ext.clone());
    }
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() > 1 {
        variants.push(parts[0].to_string());
        variants.push(parts[parts.len() - 1].to_string());
    }
    let ext_parts: Vec<&str> = without_ext.split('-').collect();
    if ext_parts.len() > 1 {
        let first = ext_parts[0].to_string();
        let last = ext_parts[ext_parts.len() - 1].to_string();
        if first != parts[0] || ext_parts.len() != parts.len() {
            variants.push(first);
        }
        if last != parts[parts.len() - 1] || ext_parts.len() != parts.len() {
            variants.push(last);
        }
    }
    let mut unique: Vec<String> = variants.into_iter().map(|v| v.to_lowercase()).collect();
    unique.sort();
    unique.dedup();
    unique
}

// ============================================================================
// Fuzzy Window Matching (pas-fuzzy-search)
// ============================================================================
/// Fuzzy matching for windows using pas-fuzzy-search (Niri-specific)
pub async fn find_window_with_heuristics(
    adapter: &NiriAdapter,
    binary: &str,
) -> Result<Option<String>> {
    zummon_debug!(
        "Applying pas-fuzzy-search heuristics to find window for: {}",
        binary
    );
    let windows = adapter.get_windows_json().await?;
    let mut candidates: Vec<String> = windows.iter().filter_map(|w| w.app_id.clone()).collect();
    // Windows with an empty app_id (WebKitGTK/Tauri on Wayland) are invisible
    // to app_id matching; include their titles as candidates so they can still
    // be found by fuzzy name matching.
    for window in &windows {
        if window
            .app_id
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            if let Some(title) = &window.title {
                candidates.push(title.clone());
            }
        }
    }
    if candidates.is_empty() {
        return Ok(None);
    }
    let binary_name = std::path::Path::new(binary)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let variants = generate_variants(&binary_name);
    zummon_debug!("Testing variants: {:?}", variants);
    let mut best_match = None;
    let mut best_score = 0.0;

    // FIX: Score the candidate against the variant, not the variant against itself!
    for candidate in &candidates {
        for variant in &variants {
            let engine = pas_fuzzy_search::PasFuzzySearch::new(variant.to_lowercase());
            let score = engine.score(candidate);
            if score > best_score {
                best_score = score;
                best_match = Some((candidate, score));
            }
        }
    }

    if let Some((app_id, score)) = best_match {
        zummon_debug!("Best fuzzy match: '{}' with score {:.3}", &app_id, score);
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
