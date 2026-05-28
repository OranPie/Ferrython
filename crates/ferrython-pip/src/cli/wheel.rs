use crate::metadata::PackageMetadata;

pub(super) fn build_wheel(src: &str, wheel_dir: &str, quiet: bool) -> Result<(), String> {
    let src_path = std::path::Path::new(src);
    let pyproject_path = src_path.join("pyproject.toml");

    if !pyproject_path.exists() {
        return Err(format!("No pyproject.toml found in {}", src_path.display()));
    }

    let pyproj = ferrython_toolchain::pyproject::parse_pyproject(&pyproject_path)?;
    let name = pyproj
        .name()
        .ok_or("No project name in pyproject.toml")?
        .to_string();
    let version = pyproj.version().unwrap_or("0.0.0").to_string();

    let normalized_name = name.replace('-', "_").replace('.', "_");
    let wheel_name = format!("{}-{}-py3-none-any.whl", normalized_name, version);

    let out_dir = std::path::Path::new(wheel_dir);
    std::fs::create_dir_all(out_dir).map_err(|e| format!("Cannot create output dir: {}", e))?;

    let wheel_path = out_dir.join(&wheel_name);

    let file =
        std::fs::File::create(&wheel_path).map_err(|e| format!("Cannot create wheel: {}", e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let pkg_dir = src_path.join(&normalized_name);
    let alt_pkg_dir = src_path.join("src").join(&normalized_name);
    let source_dir = if pkg_dir.exists() {
        Some(pkg_dir)
    } else if alt_pkg_dir.exists() {
        Some(alt_pkg_dir)
    } else {
        None
    };

    let mut file_count = 0;
    if let Some(ref pkg) = source_dir {
        add_dir_to_zip(&mut zip, pkg, &normalized_name, &options, &mut file_count)
            .map_err(|e| format!("Failed adding sources: {}", e))?;
    } else {
        let single = src_path.join(format!("{}.py", normalized_name));
        if single.exists() {
            let content =
                std::fs::read_to_string(&single).map_err(|e| format!("Read error: {}", e))?;
            zip.start_file(format!("{}.py", normalized_name), options)
                .map_err(|e| format!("Zip error: {}", e))?;
            use std::io::Write;
            zip.write_all(content.as_bytes())
                .map_err(|e| format!("Write error: {}", e))?;
            file_count += 1;
        } else {
            return Err(format!(
                "No package directory '{}' or '{}.py' found",
                normalized_name, normalized_name
            ));
        }
    }

    let dist_info_prefix = format!("{}-{}.dist-info", normalized_name, version);

    let pkg_meta = PackageMetadata::from_pyproject(&pyproj);
    let metadata = pkg_meta.render();

    zip.start_file(format!("{}/METADATA", dist_info_prefix), options)
        .map_err(|e| format!("Zip error: {}", e))?;
    {
        use std::io::Write;
        zip.write_all(metadata.as_bytes())
            .map_err(|e| format!("Write error: {}", e))?;
    }

    let wheel_metadata =
        "Wheel-Version: 1.0\nGenerator: ferrypip\nRoot-Is-Purelib: true\nTag: py3-none-any\n";
    zip.start_file(format!("{}/WHEEL", dist_info_prefix), options)
        .map_err(|e| format!("Zip error: {}", e))?;
    {
        use std::io::Write;
        zip.write_all(wheel_metadata.as_bytes())
            .map_err(|e| format!("Write error: {}", e))?;
    }

    zip.start_file(format!("{}/RECORD", dist_info_prefix), options)
        .map_err(|e| format!("Zip error: {}", e))?;

    if let Some(scripts) = pyproj.scripts() {
        let mut entry_points = String::from("[console_scripts]\n");
        for (name, entry) in scripts {
            entry_points.push_str(&format!("{} = {}\n", name, entry));
        }
        zip.start_file(format!("{}/entry_points.txt", dist_info_prefix), options)
            .map_err(|e| format!("Zip error: {}", e))?;
        use std::io::Write;
        zip.write_all(entry_points.as_bytes())
            .map_err(|e| format!("Write error: {}", e))?;
    }

    zip.finish()
        .map_err(|e| format!("Zip finalize error: {}", e))?;

    if !quiet {
        println!("Built wheel: {} ({} source files)", wheel_name, file_count);
        println!("  Output: {}", wheel_path.display());
    }
    Ok(())
}

fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<std::fs::File>,
    dir: &std::path::Path,
    prefix: &str,
    options: &zip::write::SimpleFileOptions,
    count: &mut usize,
) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("Cannot read {}: {}", dir.display(), e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') || name == "__pycache__" || name.ends_with(".pyc") {
            continue;
        }

        let zip_path = format!("{}/{}", prefix, name);

        if path.is_dir() {
            add_dir_to_zip(zip, &path, &zip_path, options, count)?;
        } else if name.ends_with(".py")
            || name.ends_with(".pyi")
            || name.ends_with(".json")
            || name.ends_with(".txt")
            || name.ends_with(".cfg")
            || name.ends_with(".toml")
        {
            let content =
                std::fs::read(&path).map_err(|e| format!("Read {}: {}", path.display(), e))?;
            zip.start_file(&zip_path, *options)
                .map_err(|e| format!("Zip entry {}: {}", zip_path, e))?;
            use std::io::Write;
            zip.write_all(&content)
                .map_err(|e| format!("Write {}: {}", zip_path, e))?;
            *count += 1;
        }
    }
    Ok(())
}
