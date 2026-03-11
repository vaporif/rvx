#[allow(dead_code)]
pub fn current_target() -> &'static str {
    env!("RVX_TARGET")
}

#[allow(dead_code)]
pub fn target_variants() -> Vec<String> {
    let primary = current_target().to_string();
    let mut variants = vec![primary.clone()];

    if primary.contains("linux") && primary.contains("gnu") {
        variants.push(primary.replace("gnu", "musl"));
    } else if primary.contains("linux") && primary.contains("musl") {
        variants.push(primary.replace("musl", "gnu"));
    }

    variants
}

#[allow(dead_code)]
pub fn binary_ext() -> &'static str {
    if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_target_is_not_empty() {
        let t = current_target();
        assert!(!t.is_empty());
        assert!(
            t.contains("linux") || t.contains("darwin") || t.contains("windows"),
            "unexpected target: {t}"
        );
    }

    #[test]
    fn test_binary_ext() {
        let ext = binary_ext();
        if cfg!(target_os = "windows") {
            assert_eq!(ext, ".exe");
        } else {
            assert_eq!(ext, "");
        }
    }

    #[test]
    fn test_target_variants_includes_primary() {
        let variants = target_variants();
        assert!(!variants.is_empty());
        assert_eq!(variants[0], current_target());
    }
}
