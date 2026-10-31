//! Directory choice for cached speech, independent of filesystem and Tauri APIs.

use std::path::Path;

/// `resource_dir` is present only when its manifest exists. Source phrases are
/// allowed only in debug builds, even if a source directory was supplied.
pub fn directory<'a>(
    resource_dir: Option<&'a Path>,
    source_dir: Option<&'a Path>,
    debug: bool,
) -> Option<&'a Path> {
    resource_dir.or(source_dir.filter(|_| debug))
}

#[cfg(test)]
mod tests {
    use super::directory;
    use std::path::Path;

    #[test]
    fn resource_manifest_wins_in_debug_and_release() {
        let resource = Path::new("installed/phrases");
        let source = Path::new("source/assets/phrases");
        for debug in [false, true] {
            assert_eq!(
                directory(Some(resource), Some(source), debug),
                Some(resource)
            );
        }
    }

    #[test]
    fn missing_resource_manifest_allows_source_only_in_debug() {
        let source = Path::new("source/assets/phrases");
        assert_eq!(directory(None, Some(source), true), Some(source));
        assert_eq!(directory(None, Some(source), false), None);
        assert_eq!(directory(None, None, true), None);
        assert_eq!(directory(None, None, false), None);
    }
}
