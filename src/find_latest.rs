// src/find_latest.rs - 4-phase executable discovery: wax globbing → pas-fuzzy-search → exhaustive fallback
use crate::zummon_debug;
use anyhow::{Result, anyhow};
use pas_fuzzy_search::PasFuzzySearch;
use semver::Version;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::fs as async_fs;
use wax::Glob;
use wax::walk::Entry;

/// Represents a candidate executable with all metadata needed for scoring
#[derive(Debug, Clone)]
struct Candidate {
    path: PathBuf,
    stem: String,
    score: u32,
    version: Option<Version>,
    mod_time: Option<SystemTime>,
    stem_len: usize,
    phase: u8, // 1=glob, 2=fuzzy, 3=multi_fuzzy, 4=exhaustive
}

/// Main entry point: resolve the best executable matching `pattern` in `versioned_path`
pub async fn resolve_latest(
    versioned_path: &Path,
    app_pattern: &str,
    use_mod_time: bool,
) -> Result<String> {
    zummon_debug!(
        "[find_latest] resolve_latest: path='{}', pattern='{}', use_mod={}",
        versioned_path.display(),
        app_pattern,
        use_mod_time
    );

    let (search_dir, dir_pattern) = if versioned_path.exists() {
        (versioned_path.to_path_buf(), None)
    } else {
        let mut parent = versioned_path.to_path_buf();
        let mut pattern = None;

        while let Some(p) = parent.parent() {
            if p.exists() {
                let relative = versioned_path.strip_prefix(p).unwrap_or(versioned_path);
                pattern = relative
                    .components()
                    .next()
                    .map(|c| c.as_os_str().to_string_lossy().to_string());
                parent = p.to_path_buf();
                break;
            }
            parent = p.to_path_buf();
        }

        if !parent.exists() {
            return Err(anyhow!(
                "Could not find valid parent directory in path: {}",
                versioned_path.display()
            ));
        }

        (parent, pattern)
    };

    zummon_debug!(
        "[find_latest] search_dir='{}', dir_pattern={:?}",
        search_dir.display(),
        dir_pattern
    );
    let pattern = dir_pattern.as_deref().unwrap_or(app_pattern);

    // PHASE 1: Wax glob patterns (deterministic, high-confidence)
    match phase1_wax_glob(&search_dir, pattern, use_mod_time).await {
        Ok(Some(candidate)) => {
            zummon_debug!(
                "[find_latest] ✓ PHASE 1 (glob) BEST GUESS: {} (score={}, path={})",
                candidate.path.display(),
                candidate.score,
                candidate.path.display()
            );
            return Ok(candidate.path.to_string_lossy().to_string());
        }
        Ok(None) => {
            zummon_debug!("[find_latest] PHASE 1 (glob): no matches found, best guess: NONE");
        }
        Err(e) => {
            zummon_debug!(
                "[find_latest] PHASE 1 (glob) error: {}, proceeding to next phase",
                e
            );
        }
    }

    // PHASE 2: pas-fuzzy-search focused matching
    match phase2_fuzzy(&search_dir, pattern, use_mod_time).await {
        Ok(Some(candidate)) => {
            zummon_debug!(
                "[find_latest] ✓ PHASE 2 (fuzzy) BEST GUESS: {} (score={}, path={})",
                candidate.path.display(),
                candidate.score,
                candidate.path.display()
            );
            return Ok(candidate.path.to_string_lossy().to_string());
        }
        Ok(None) => {
            zummon_debug!(
                "[find_latest] PHASE 2 (fuzzy): no viable matches (threshold ≥ 80), best guess: NONE"
            );
        }
        Err(e) => {
            zummon_debug!(
                "[find_latest] PHASE 2 (fuzzy) error: {}, proceeding to next phase",
                e
            );
        }
    }

    // PHASE 3: Multi-tier fuzzy matching
    match phase3_multi_tier_fuzzy(&search_dir, pattern, use_mod_time).await {
        Ok(Some(candidate)) => {
            zummon_debug!(
                "[find_latest] ✓ PHASE 3 (multi-fuzzy) BEST GUESS: {} (score={}, path={})",
                candidate.path.display(),
                candidate.score,
                candidate.path.display()
            );
            return Ok(candidate.path.to_string_lossy().to_string());
        }
        Ok(None) => {
            zummon_debug!(
                "[find_latest] PHASE 3 (multi-fuzzy): no viable matches (threshold ≥ 45), best guess: NONE"
            );
        }
        Err(e) => {
            zummon_debug!(
                "[find_latest] PHASE 3 (multi-fuzzy) error: {}, proceeding to next phase",
                e
            );
        }
    }

    // PHASE 4: Exhaustive fallback
    match phase4_exhaustive_fallback(&search_dir, pattern, use_mod_time).await {
        Ok(Some(candidate)) => {
            zummon_debug!(
                "[find_latest] ✓ PHASE 4 (exhaustive) BEST GUESS: {} (score={}, path={})",
                candidate.path.display(),
                candidate.score,
                candidate.path.display()
            );
            return Ok(candidate.path.to_string_lossy().to_string());
        }
        Ok(None) => {
            zummon_debug!(
                "[find_latest] PHASE 4 (exhaustive): no matches found even with relaxed thresholds, best guess: NONE"
            );
        }
        Err(e) => {
            zummon_debug!("[find_latest] PHASE 4 (exhaustive) error: {}", e);
        }
    }

    if tracing::enabled!(tracing::Level::DEBUG) {
        log_all_executables(&search_dir, pattern).await;
    }
    Err(anyhow!(
        "No executable matching '{}' found in {} after exhaustive 4-phase search",
        pattern,
        search_dir.display()
    ))
}

async fn phase1_wax_glob(
    search_dir: &Path,
    pattern: &str,
    use_mod_time: bool,
) -> Result<Option<Candidate>> {
    zummon_debug!("[find_latest] PHASE 1: wax glob patterns for '{}'", pattern);

    let glob_patterns = [
        format!("**/*{}{{.AppImage,.app,.exe,.sh,.bin,.run}}", pattern),
        format!("**/*{}*", pattern),
        format!("**/*{}-*", pattern),
        format!("**/*{}v*", pattern),
        format!("**/*{}.*.AppImage", pattern),
        format!("**/*{}-*-x86_64.AppImage", pattern),
        format!("**/*{}-*-aarch64.AppImage", pattern),
    ];

    let mut candidates = Vec::new();

    for glob_str in &glob_patterns {
        zummon_debug!("[find_latest] PHASE 1: trying glob '{}'", glob_str);

        let glob = match Glob::new(glob_str) {
            Ok(g) => g,
            Err(e) => {
                zummon_debug!("[find_latest] PHASE 1: invalid glob '{}': {}", glob_str, e);
                continue;
            }
        };

        for entry in glob.walk(search_dir) {
            if let Ok(entry) = entry {
                // FIX: Use entry.path() first, then standard Path::file_name()
                let path = entry.path().to_path_buf();
                if !path.is_file() {
                    continue;
                }
                let filename = path.file_name().unwrap_or_default().to_string_lossy();
                if !is_executable(&path, &filename.to_lowercase()).await {
                    continue;
                }
                if let Some(c) = build_glob_candidate(&path, pattern).await {
                    candidates.push(c);
                }
            }
        }

        let bin_dir = search_dir.join("bin");
        if bin_dir.exists() {
            for entry in glob.walk(&bin_dir) {
                if let Ok(entry) = entry {
                    // FIX: Use entry.path() first, then standard Path::file_name()
                    let path = entry.path().to_path_buf();
                    if !path.is_file() {
                        continue;
                    }
                    let filename = path.file_name().unwrap_or_default().to_string_lossy();
                    if !is_executable(&path, &filename.to_lowercase()).await {
                        continue;
                    }
                    if let Some(c) = build_glob_candidate(&path, pattern).await {
                        candidates.push(c);
                    }
                }
            }
        }
    }

    if candidates.is_empty() {
        return Ok(None);
    }

    zummon_debug!(
        "[find_latest] PHASE 1: found {} glob matches",
        candidates.len()
    );

    candidates.sort_by(|a, b| {
        match (&b.version, &a.version) {
            (Some(bv), Some(av)) => match bv.cmp(av) {
                std::cmp::Ordering::Equal => {}
                ord => return ord,
            },
            (Some(_), None) => return std::cmp::Ordering::Less,
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            (None, None) => {}
        }
        if use_mod_time
            && let (Some(bt), Some(at)) = (&b.mod_time, &a.mod_time) { return bt.cmp(at) }
        a.stem_len.cmp(&b.stem_len)
    });

    if let Some(best) = candidates.first() {
        zummon_debug!(
            "[find_latest] PHASE 1 BEST GUESS: {} (score={}, ver={:?})",
            best.path.display(),
            best.score,
            best.version
        );
    }

    Ok(candidates.first().cloned())
}

async fn phase2_fuzzy(
    search_dir: &Path,
    pattern: &str,
    use_mod_time: bool,
) -> Result<Option<Candidate>> {
    zummon_debug!("[find_latest] PHASE 2: pas-fuzzy-search for '{}'", pattern);

    let mut candidates = Vec::new();
    let pattern_lower = pattern.to_lowercase();
    let engine = PasFuzzySearch::new(&pattern_lower);

    for dir in [search_dir, &search_dir.join("bin")] {
        if !dir.exists() {
            continue;
        }
        let mut entries = async_fs::read_dir(dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let filename = path.file_name().unwrap_or_default().to_string_lossy();
            let filename_lower = filename.to_lowercase();
            if !is_executable(&path, &filename_lower).await {
                continue;
            }

            let stem = extract_stem(&filename_lower);
            let score = engine.score(&stem);
            if score < 0.80 {
                continue;
            }

            let scaled = (score * 100.0) as u32;
            let version = extract_version(&filename);
            let mod_time = async_fs::metadata(&path)
                .await
                .ok()
                .and_then(|m| m.modified().ok());

            candidates.push(Candidate {
                path: path.to_path_buf(),
                stem: stem.clone(),
                score: scaled,
                version,
                mod_time,
                stem_len: stem.len(),
                phase: 2,
            });
        }
    }

    if candidates.is_empty() {
        return Ok(None);
    }

    zummon_debug!(
        "[find_latest] PHASE 2: found {} fuzzy matches (score ≥ 80)",
        candidates.len()
    );
    candidates.sort_by(|a, b| sort_candidates(a, b, use_mod_time));

    if let Some(best) = candidates.first() {
        zummon_debug!(
            "[find_latest] PHASE 2 BEST GUESS: {} (score={}, ver={:?})",
            best.path.display(),
            best.score,
            best.version
        );
    }

    Ok(candidates.first().cloned())
}

async fn phase3_multi_tier_fuzzy(
    search_dir: &Path,
    pattern: &str,
    use_mod_time: bool,
) -> Result<Option<Candidate>> {
    zummon_debug!("[find_latest] PHASE 3: multi-tier fuzzy for '{}'", pattern);

    let mut candidates = Vec::new();
    let pattern_lower = pattern.to_lowercase();

    for dir in [search_dir, &search_dir.join("bin")] {
        if !dir.exists() {
            continue;
        }
        let mut entries = async_fs::read_dir(dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let filename = path.file_name().unwrap_or_default().to_string_lossy();
            let filename_lower = filename.to_lowercase();
            if !is_executable(&path, &filename_lower).await {
                continue;
            }

            if let Some(c) = build_fuzzy_candidate(&path, &pattern_lower, 3).await {
                candidates.push(c);
            }
        }
    }

    if candidates.is_empty() {
        return Ok(None);
    }

    zummon_debug!(
        "[find_latest] PHASE 3: found {} fuzzy candidates",
        candidates.len()
    );

    let thresholds = [85, 65, 45];
    for threshold in thresholds {
        let mut viable: Vec<&Candidate> =
            candidates.iter().filter(|c| c.score >= threshold).collect();
        if !viable.is_empty() {
            viable.sort_by(|a, b| sort_candidates(a, b, use_mod_time));
            zummon_debug!(
                "[find_latest] PHASE 3: threshold {} succeeded: {} viable",
                threshold,
                viable.len()
            );
            if let Some(best) = viable.first() {
                zummon_debug!(
                    "[find_latest] PHASE 3 BEST GUESS: {} (score={}, ver={:?})",
                    best.path.display(),
                    best.score,
                    best.version
                );
            }
            return Ok(viable.first().cloned().cloned());
        }
        zummon_debug!(
            "[find_latest] PHASE 3: threshold {} failed: 0 viable",
            threshold
        );
    }

    candidates.sort_by(|a, b| sort_candidates(a, b, use_mod_time));
    if let Some(best) = candidates.first() {
        zummon_debug!(
            "[find_latest] PHASE 3 BEST GUESS (below threshold): {} (score={})",
            best.path.display(),
            best.score
        );
    }

    Ok(None)
}

async fn build_fuzzy_candidate(path: &Path, pattern: &str, phase: u8) -> Option<Candidate> {
    let filename = path.file_name()?.to_string_lossy();
    let filename_lower = filename.to_lowercase();
    let stem = extract_stem(&filename_lower);

    let (score, _tier) = compute_multi_tier_score(&stem, pattern);
    if score == 0 {
        return None;
    }

    let version = extract_version(&filename);
    let mod_time = async_fs::metadata(path)
        .await
        .ok()
        .and_then(|m| m.modified().ok());

    Some(Candidate {
        path: path.to_path_buf(),
        stem: stem.clone(),
        score,
        version,
        mod_time,
        stem_len: stem.len(),
        phase,
    })
}

fn compute_multi_tier_score(stem: &str, pattern: &str) -> (u32, u8) {
    let s = stem.to_lowercase();
    let p = pattern.to_lowercase();

    if s == p {
        return (100, 1);
    }
    if s.starts_with(&p) || s.ends_with(&p) {
        return (90, 1);
    }
    if s.contains(&p) {
        return (80, 1);
    }

    let engine = PasFuzzySearch::new(&p);
    let fuzzy_score = engine.score(&s);
    let fuzzy_points = if fuzzy_score >= 0.85 {
        70
    } else if fuzzy_score >= 0.70 {
        55
    } else if fuzzy_score >= 0.55 {
        40
    } else {
        0
    };
    if fuzzy_points > 0 {
        return (fuzzy_points, 2);
    }

    let s_clean: String = s.chars().filter(|c| c.is_alphanumeric()).collect();
    let p_clean: String = p.chars().filter(|c| c.is_alphanumeric()).collect();
    if s_clean == p_clean {
        return (85, 3);
    }
    if s_clean.starts_with(&p_clean) || s_clean.ends_with(&p_clean) {
        return (75, 3);
    }
    if s_clean.contains(&p_clean) {
        return (65, 3);
    }

    let clean_engine = PasFuzzySearch::new(&p_clean);
    let clean_fuzzy = clean_engine.score(&s_clean);
    let clean_points = if clean_fuzzy >= 0.80 {
        60
    } else if clean_fuzzy >= 0.65 {
        45
    } else {
        0
    };
    if clean_points > 0 {
        return (clean_points + 10, 3);
    }

    (0, 0)
}

async fn phase4_exhaustive_fallback(
    search_dir: &Path,
    pattern: &str,
    use_mod_time: bool,
) -> Result<Option<Candidate>> {
    zummon_debug!(
        "[find_latest] PHASE 4: exhaustive fallback for '{}'",
        pattern
    );

    let mut candidates = Vec::new();
    let pattern_lower = pattern.to_lowercase();
    let p_clean: String = pattern_lower
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    let engine = PasFuzzySearch::new(&p_clean);

    for dir in [search_dir, &search_dir.join("bin")] {
        if !dir.exists() {
            continue;
        }
        let mut entries = async_fs::read_dir(dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let filename = path.file_name().unwrap_or_default().to_string_lossy();
            let filename_lower = filename.to_lowercase();
            if !is_executable(&path, &filename_lower).await {
                continue;
            }

            let stem = extract_stem(&filename_lower);
            let s_clean: String = stem.chars().filter(|c| c.is_alphanumeric()).collect();
            let score = (engine.score(&s_clean) * 100.0) as u32;
            if score < 30 {
                continue;
            }

            let version = extract_version(&filename);
            let mod_time = async_fs::metadata(&path)
                .await
                .ok()
                .and_then(|m| m.modified().ok());

            candidates.push(Candidate {
                path: path.to_path_buf(),
                stem: stem.clone(),
                score: score + 20,
                version,
                mod_time,
                stem_len: stem.len(),
                phase: 4,
            });
        }
    }

    if candidates.is_empty() {
        return Ok(None);
    }

    zummon_debug!(
        "[find_latest] PHASE 4: found {} exhaustive candidates",
        candidates.len()
    );
    candidates.sort_by(|a, b| sort_candidates(a, b, use_mod_time));

    if let Some(best) = candidates.first() {
        zummon_debug!(
            "[find_latest] PHASE 4 BEST GUESS: {} (score={}, ver={:?})",
            best.path.display(),
            best.score,
            best.version
        );
    }

    Ok(candidates.first().cloned())
}

async fn build_glob_candidate(path: &Path, pattern: &str) -> Option<Candidate> {
    let filename = path.file_name()?.to_string_lossy();
    let stem = extract_stem(&filename.to_lowercase());

    let base_score = if stem == pattern.to_lowercase() {
        100
    } else if stem.starts_with(pattern) || stem.ends_with(pattern) {
        90
    } else {
        80
    };

    let version = extract_version(&filename);
    let mod_time = async_fs::metadata(path)
        .await
        .ok()
        .and_then(|m| m.modified().ok());

    Some(Candidate {
        path: path.to_path_buf(),
        stem: stem.clone(),
        score: base_score,
        version,
        mod_time,
        stem_len: stem.len(),
        phase: 1,
    })
}

async fn is_executable(path: &Path, _filename_lower: &str) -> bool {
    let exec_exts: &[&str] = if cfg!(windows) {
        &["exe", "bat", "cmd", "ps1", "msi"]
    } else if cfg!(target_os = "macos") {
        &["app", "command", "sh", "bash", "zsh"]
    } else {
        &["appimage", "sh", "bash", "zsh", "bin", "run", "app"]
    };

    if let Some(ext) = path.extension().and_then(|e| e.to_str())
        && exec_exts.iter().any(|&e| ext.to_lowercase() == e) {
            return true;
        }

    #[cfg(unix)]
    {
        if let Ok(metadata) = async_fs::metadata(path).await {
            use std::os::unix::fs::PermissionsExt;
            return metadata.permissions().mode() & 0o111 != 0;
        }
    }
    false
}

fn extract_stem(filename_lower: &str) -> String {
    let extensions = [
        ".appimage",
        ".app",
        ".exe",
        ".bat",
        ".cmd",
        ".ps1",
        ".msi",
        ".sh",
        ".bash",
        ".zsh",
        ".bin",
        ".run",
        ".command",
        ".tar.gz",
        ".tar.bz2",
        ".zip",
    ];
    let mut stem = filename_lower.to_string();
    for ext in extensions {
        if stem.ends_with(ext) {
            stem.truncate(stem.len() - ext.len());
            break;
        }
    }
    stem
}

fn extract_version(s: &str) -> Option<Version> {
    use regex::Regex;
    let re = Regex::new(r"(\d+\.\d+(?:\.\d+)*(?:-\d+)?)").ok()?;
    re.find(s).and_then(|m| Version::parse(m.as_str()).ok())
}

fn sort_candidates(a: &Candidate, b: &Candidate, use_mod_time: bool) -> std::cmp::Ordering {
    match b.score.cmp(&a.score) {
        std::cmp::Ordering::Equal => {}
        ord => return ord,
    }
    match a.phase.cmp(&b.phase) {
        std::cmp::Ordering::Equal => {}
        ord => return ord,
    }
    match (&b.version, &a.version) {
        (Some(bv), Some(av)) => match bv.cmp(av) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        },
        (Some(_), None) => return std::cmp::Ordering::Less,
        (None, Some(_)) => return std::cmp::Ordering::Greater,
        (None, None) => {}
    }
    if (use_mod_time || a.version == b.version)
        && let (Some(bt), Some(at)) = (&b.mod_time, &a.mod_time) { match bt.cmp(at) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        } }
    match a.stem_len.cmp(&b.stem_len) {
        std::cmp::Ordering::Equal => {}
        ord => return ord,
    }
    a.path.file_name().cmp(&b.path.file_name())
}

async fn log_all_executables(search_dir: &Path, _pattern: &str) {
    zummon_debug!(
        "[find_latest] EXHAUSTIVE DEBUG: all executables in {}",
        search_dir.display()
    );
    for dir in [search_dir, &search_dir.join("bin")] {
        if !dir.exists() {
            continue;
        }
        let mut entries = async_fs::read_dir(dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let filename = path.file_name().unwrap_or_default().to_string_lossy();
            let stem = extract_stem(&filename.to_lowercase());
            let version = extract_version(&filename);
            zummon_debug!(
                "[find_latest]   file: '{}' stem='{}' ver={:?}",
                filename,
                stem,
                version
            );
        }
    }
}
