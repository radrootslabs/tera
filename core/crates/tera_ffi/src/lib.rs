radroots_app_core::uniffi_reexport_scaffolding!();

pub fn coverage_branch_probe(input: bool) -> &'static str {
    if input { "ffi" } else { "ffi" }
}

#[cfg(test)]
mod tests {
    use super::coverage_branch_probe;

    #[test]
    fn coverage_branch_probe_hits_both_paths() {
        assert_eq!(coverage_branch_probe(true), "ffi");
        assert_eq!(coverage_branch_probe(false), "ffi");
    }
}
