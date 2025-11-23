use std::path::Path;

pub fn make_output_name(src: &str) -> String {
    if src.ends_with(".tap") {
        src.to_string()
    } else {
        format!("{}.tap", src)
    }
}

pub fn make_input_name(src: &str) -> Option<String> {
    if src == "-" {
        return None;
    }
    if src.contains('/') {
        Some(src.to_string())
    } else {
        Some(format!("/dev/{}", src))
    }
}

pub fn device_token_candidates(input: &Option<String>) -> Vec<String> {
    let Some(raw) = input else {
        return Vec::new();
    };
    let sanitized = raw.trim_end_matches('/');
    if sanitized.is_empty() || sanitized == "/dev" {
        return Vec::new();
    }

    let Some(name) = Path::new(sanitized).file_name().and_then(|p| p.to_str()) else {
        return Vec::new();
    };

    let lower = name.to_lowercase();
    if lower.is_empty() {
        return Vec::new();
    }

    let mut tokens = vec![lower.clone()];
    if let Some(stripped) = lower.strip_prefix('n') {
        if !stripped.is_empty() {
            tokens.push(stripped.to_string());
        }
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_output_name() {
        assert_eq!(make_output_name("test"), "test.tap");
        assert_eq!(make_output_name("test.tap"), "test.tap");
    }

    #[test]
    fn test_make_input_name() {
        assert_eq!(make_input_name("-"), None);
        assert_eq!(make_input_name("/dev/rmt0"), Some("/dev/rmt0".to_string()));
        assert_eq!(make_input_name("rmt0"), Some("/dev/rmt0".to_string()));
        assert_eq!(make_input_name("./file"), Some("./file".to_string()));
    }

    #[test]
    fn test_device_token_candidates() {
        assert!(device_token_candidates(&None).is_empty());
        assert_eq!(
            device_token_candidates(&Some("/dev/nst0".to_string())),
            vec!["nst0".to_string(), "st0".to_string()]
        );
        assert_eq!(
            device_token_candidates(&Some("/dev/st1".to_string())),
            vec!["st1".to_string()]
        );
        assert!(device_token_candidates(&Some("/dev/".to_string())).is_empty());
    }
}
