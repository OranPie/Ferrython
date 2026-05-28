use crate::{pypi, registry};

pub(super) fn show_package(
    name: &str,
    site_packages: &str,
    show_files: bool,
) -> Result<(), String> {
    if let Some(info) = registry::get_installed(name, site_packages) {
        println!("Name: {}", info.name);
        println!("Version: {}", info.version);
        if let Some(ref summary) = info.summary {
            println!("Summary: {}", summary);
        }
        if let Some(ref home_page) = info.home_page {
            println!("Home-page: {}", home_page);
        }
        if let Some(ref author) = info.author {
            println!("Author: {}", author);
        }
        if let Some(ref license) = info.license {
            println!("License: {}", license);
        }
        if let Some(ref requires_python) = info.requires_python {
            println!("Requires-Python: {}", requires_python);
        }
        println!("Location: {}", site_packages);
        print_requires(info.requires.as_ref());
        print_required_by(&info.name, site_packages);
        println!("Installer: ferrypip");
        if show_files {
            println!("Files:");
            for file in &info.files {
                println!("  {}", file);
            }
            println!("  ({} file(s))", info.files.len());
        }
        return Ok(());
    }

    match pypi::fetch_package_info(name, None) {
        Ok(info) => {
            println!("Name: {} (not installed)", info.name);
            println!("Version: {} (latest)", info.version);
            if !info.summary.is_empty() {
                println!("Summary: {}", info.summary);
            }
            if !info.author.is_empty() {
                println!("Author: {}", info.author);
            }
            if !info.license.is_empty() {
                println!("License: {}", info.license);
            }
            if !info.requires_dist.is_empty() {
                println!("Requires: {}", info.requires_dist.join(", "));
            }
            println!("\nTo install: ferrypip install {}", name);
            Ok(())
        }
        Err(_) => {
            let hint = suggest_similar_package(name)
                .map(|similar| format!("\nDid you mean: {}?\n", similar))
                .unwrap_or_default();
            Err(format!(
                "Package '{}' is not installed and was not found on PyPI.\n\
                 {}Hint: Check the package name spelling or search with: ferrypip search {}",
                name, hint, name
            ))
        }
    }
}

fn print_requires(requires: Option<&Vec<String>>) {
    if let Some(requires) = requires {
        let dep_names: Vec<String> = requires
            .iter()
            .map(|req| req.split(';').next().unwrap_or(req).trim().to_string())
            .collect();
        println!("Requires: {}", dep_names.join(", "));
    } else {
        println!("Requires: (none)");
    }
}

fn print_required_by(name: &str, site_packages: &str) {
    let normalized_name = normalize_name(name);
    let required_by: Vec<String> = registry::list_installed(site_packages)
        .iter()
        .filter(|pkg| {
            pkg.requires.as_ref().map_or(false, |reqs| {
                reqs.iter().any(|req| {
                    let dep = req.split_whitespace().next().unwrap_or(req);
                    let dep = dep
                        .split(&['>', '<', '=', '!', '~', ';', '(', '['][..])
                        .next()
                        .unwrap_or(dep);
                    normalize_name(dep) == normalized_name
                })
            })
        })
        .map(|pkg| pkg.name.clone())
        .collect();
    if required_by.is_empty() {
        println!("Required-by: (none)");
    } else {
        println!("Required-by: {}", required_by.join(", "));
    }
}

pub(super) fn search_pypi(query: &str) -> Result<(), String> {
    let results = pypi::search(query).map_err(|e| format!("Search failed: {}", e))?;
    if !results.is_empty() {
        print_search_results(&results);
        return Ok(());
    }

    let variations = vec![
        query.replace(' ', "-"),
        query.replace(' ', "_"),
        query.replace('_', "-"),
        query.replace('-', "_"),
        format!("python-{}", query),
        format!("py{}", query),
    ];
    let mut found = false;
    let mut seen = std::collections::HashSet::new();
    seen.insert(query.to_lowercase());
    for variant in &variations {
        if !seen.insert(variant.to_lowercase()) {
            continue;
        }
        if let Ok(results) = pypi::search(variant) {
            print_search_results(&results);
            found |= !results.is_empty();
        }
    }

    if !found {
        println!("No packages found matching '{}'.", query);
        println!("Hint: Try browsing https://pypi.org/search/?q={}", query);
    }
    Ok(())
}

fn print_search_results(results: &[(String, String, String)]) {
    for (name, version, summary) in results {
        println!("{} ({}) - {}", name, version, summary);
    }
}

fn suggest_similar_package(name: &str) -> Option<String> {
    let variations = vec![
        name.replace('_', "-"),
        name.replace('-', "_"),
        format!("python-{}", name),
        format!("py{}", name),
        format!("{}3", name),
    ];
    for variant in &variations {
        if variant == name {
            continue;
        }
        if let Ok(results) = pypi::search(variant) {
            if !results.is_empty() {
                return Some(results[0].0.clone());
            }
        }
    }
    None
}

pub(super) fn find_closest_name(needle: &str, haystack: &[&str]) -> Option<String> {
    let needle_lower = needle.to_lowercase();
    let mut best: Option<(usize, String)> = None;
    for &candidate in haystack {
        let dist = edit_distance(&needle_lower, &candidate.to_lowercase());
        let threshold = (needle.len() / 2).max(3);
        if dist <= threshold && (best.is_none() || dist < best.as_ref().unwrap().0) {
            best = Some((dist, candidate.to_string()));
        }
    }
    best.map(|(_, name)| name)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let mut prev = (0..=b_bytes.len()).collect::<Vec<_>>();
    let mut curr = vec![0; b_bytes.len() + 1];
    for i in 1..=a_bytes.len() {
        curr[0] = i;
        for j in 1..=b_bytes.len() {
            let cost = usize::from(a_bytes[i - 1] != b_bytes[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_bytes.len()]
}

fn normalize_name(name: &str) -> String {
    name.to_lowercase().replace('-', "_").replace('.', "_")
}
