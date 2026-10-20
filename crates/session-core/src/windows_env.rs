//! Native UTF-16 environment-name ordering, independent of locale and UTF-8 conversion.
use std::{cmp::Ordering, ffi::OsStr, os::windows::ffi::OsStrExt};
use windows_sys::Win32::Globalization::CompareStringOrdinal;

#[derive(Debug)]
pub(crate) struct EnvironmentKey(Vec<u16>);
impl EnvironmentKey {
    pub(crate) fn new(value: &OsStr) -> Self {
        Self(value.encode_wide().collect())
    }
}
impl PartialEq for EnvironmentKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for EnvironmentKey {}
impl PartialOrd for EnvironmentKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for EnvironmentKey {
    fn cmp(&self, other: &Self) -> Ordering {
        // Windows environment names are bounded well below i32::MAX. Both slices remain live.
        let result = unsafe {
            CompareStringOrdinal(
                self.0.as_ptr(),
                i32::try_from(self.0.len()).expect("environment name length"),
                other.0.as_ptr(),
                i32::try_from(other.0.len()).expect("environment name length"),
                1,
            )
        };
        assert!((1..=3).contains(&result), "valid ordinal comparison failed");
        result.cmp(&2)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, ffi::OsString, os::windows::ffi::OsStringExt};
    #[test]
    fn names_use_native_case_comparison_without_losing_utf16() {
        assert_eq!(
            EnvironmentKey::new(OsStr::new("Path")),
            EnvironmentKey::new(OsStr::new("PATH"))
        );
        assert_ne!(
            EnvironmentKey::new(OsStr::new("ß")),
            EnvironmentKey::new(OsStr::new("SS"))
        );
        let a = OsString::from_wide(&[0xd800]);
        let b = OsString::from_wide(&[0xd801]);
        let mut keys = BTreeMap::new();
        keys.insert(EnvironmentKey::new(&a), a.clone());
        keys.insert(EnvironmentKey::new(&b), b.clone());
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[&EnvironmentKey::new(&a)], a);
        assert_eq!(keys[&EnvironmentKey::new(&b)], b);
    }
}
