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
}
