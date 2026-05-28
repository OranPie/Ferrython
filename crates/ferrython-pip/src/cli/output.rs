pub(super) fn status(label: &str, message: impl std::fmt::Display, quiet: bool) {
    if !quiet {
        println!("{:<10} {}", label, message);
    }
}

pub(super) fn detail(message: impl std::fmt::Display, quiet: bool) {
    if !quiet {
        println!("  {}", message);
    }
}
