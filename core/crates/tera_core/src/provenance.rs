pub(crate) fn is_full_revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::is_full_revision;

    #[test]
    fn only_full_lowercase_git_revisions_are_accepted() {
        assert!(is_full_revision("0123456789abcdef0123456789abcdef01234567"));
        assert!(!is_full_revision("0123456"));
        assert!(!is_full_revision(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!is_full_revision(
            "0123456789ABCDEF0123456789abcdef01234567"
        ));
        assert!(!is_full_revision(
            "g123456789abcdef0123456789abcdef01234567"
        ));
    }
}
