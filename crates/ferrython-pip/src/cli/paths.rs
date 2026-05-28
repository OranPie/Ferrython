pub(super) fn default_site_packages() -> String {
    // Look for ferrython's site-packages relative to the binary
    let exe = std::env::current_exe().unwrap_or_default();
    let base = exe.parent().unwrap_or(std::path::Path::new("."));
    let site = base.join("lib").join("ferrython").join("site-packages");
    if !site.exists() {
        let _ = std::fs::create_dir_all(&site);
    }
    site.to_string_lossy().to_string()
}

pub(super) fn user_site_packages() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let site = std::path::Path::new(&home)
        .join(".local")
        .join("lib")
        .join("ferrython")
        .join("site-packages");
    if !site.exists() {
        let _ = std::fs::create_dir_all(&site);
    }
    site.to_string_lossy().to_string()
}

pub(super) fn show_debug(site_packages: &str) -> Result<(), String> {
    let exe = std::env::current_exe().unwrap_or_default();
    println!("ferrypip {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("{:<20} {}", "executable", exe.display());
    println!("{:<20} {}", "site-packages", site_packages);
    println!("{:<20} {}", "user-site-packages", user_site_packages());
    println!();
    println!("Environment:");
    for var in &["FERRYTHON_COMPAT", "PYTHONPATH", "PYTHONDONTWRITEBYTECODE"] {
        match std::env::var(var) {
            Ok(val) => println!("  {}={}", var, val),
            Err(_) => println!("  {} (unset)", var),
        }
    }
    Ok(())
}
