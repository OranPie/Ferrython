use super::CacheAction;

pub(super) fn cache_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            std::path::PathBuf::from(home).join(".cache")
        });
    base.join("ferrypip").join("wheels")
}

pub(super) fn handle_cache(action: CacheAction, _quiet: bool) -> Result<(), String> {
    let dir = cache_dir();
    match action {
        CacheAction::Dir => {
            println!("Package cache directory: {}", dir.display());
            let size = dir_size(&dir);
            println!("Cache size: {}", format_size(size));
        }
        CacheAction::Info => {
            println!("Package cache location: {}", dir.display());
            let count = count_cached(&dir);
            let size = dir_size(&dir);
            println!("Number of cached wheels: {}", count);
            println!("Cache size: {}", format_size(size));
        }
        CacheAction::List => {
            if !dir.exists() {
                println!("Cache is empty.");
                return Ok(());
            }
            let entries =
                std::fs::read_dir(&dir).map_err(|e| format!("Cannot read cache: {}", e))?;
            let mut found = false;
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".whl") || name.ends_with(".tar.gz") {
                    let meta = entry.metadata().ok();
                    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                    println!("  {} ({})", name, format_size(size));
                    found = true;
                }
            }
            if !found {
                println!("Cache is empty.");
            }
        }
        CacheAction::Purge => {
            if dir.exists() {
                let count = count_cached(&dir);
                std::fs::remove_dir_all(&dir).map_err(|e| format!("Purge failed: {}", e))?;
                println!("Removed {} cached files.", count);
            } else {
                println!("Cache is already empty.");
            }
        }
        CacheAction::Remove { pattern } => {
            if !dir.exists() {
                println!("Cache is empty.");
                return Ok(());
            }
            let pattern_lower = pattern.to_lowercase();
            let entries =
                std::fs::read_dir(&dir).map_err(|e| format!("Cannot read cache: {}", e))?;
            let mut removed = 0;
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.to_lowercase().contains(&pattern_lower) {
                    let _ = std::fs::remove_file(entry.path());
                    removed += 1;
                }
            }
            println!("Removed {} cached file(s) matching '{}'.", removed, pattern);
        }
    }
    Ok(())
}

fn dir_size(path: &std::path::Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

fn count_cached(path: &std::path::Path) -> usize {
    if !path.exists() {
        return 0;
    }
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().count())
        .unwrap_or(0)
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} kB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
