pub(super) fn compute_hashes(files: &[String], algorithm: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};

    if files.is_empty() {
        return Err("No files specified".to_string());
    }

    for file_path in files {
        let data =
            std::fs::read(file_path).map_err(|e| format!("Cannot read '{}': {}", file_path, e))?;

        let hash = match algorithm {
            "sha256" => {
                let mut hasher = Sha256::new();
                hasher.update(&data);
                format!("{:x}", hasher.finalize())
            }
            other => return Err(format!("Unsupported algorithm '{}' (use sha256)", other)),
        };

        println!("{}:", file_path);
        println!("--hash={}:{}", algorithm, hash);
    }
    Ok(())
}
