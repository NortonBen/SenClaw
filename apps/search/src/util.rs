//! Small shared helpers.

/// Truncate to at most `max` **characters**, never splitting a UTF-8 sequence.
///
/// `&s[..n]` panics on multibyte text, which every Vietnamese string is —
/// see [[utf8-preview-slice-panic]].
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_never_splits_a_multibyte_char() {
        let s = "lãi suất điều hành";
        let out = truncate_chars(s, 5);
        assert_eq!(out, "lãi s");
        assert_eq!(out.chars().count(), 5);
    }

    #[test]
    fn short_strings_pass_through_unchanged() {
        assert_eq!(truncate_chars("abc", 10), "abc");
        assert_eq!(truncate_chars("", 10), "");
    }
}
