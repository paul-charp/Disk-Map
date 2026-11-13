//! Stable folder names for per-root data (snapshots, timeline).

/// Folder-safe key for a scan root: a readable slug plus a hash of the normalised path.
pub fn root_key(root: &str) -> String {
    let norm = root.trim().trim_end_matches(['\\', '/']).to_lowercase();
    let slug: String = norm
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let slug = slug.trim_matches('_');
    let slug = if slug.len() > 40 { &slug[..40] } else { slug };
    format!("{slug}-{:08x}", fnv1a(norm.as_bytes()))
}

fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

#[cfg(test)]
mod tests {
    use super::root_key;

    #[test]
    fn keys_ignore_case_and_trailing_separator() {
        assert_eq!(root_key(r"C:\"), root_key("c:"));
        assert!(root_key(r"C:\Users").starts_with("c__users-"));
        assert_ne!(root_key(r"C:\a b"), root_key(r"C:\a_b"));
    }
}
