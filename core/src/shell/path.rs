//! Utilidades de comparación de rutas y nombres para Windows
//!
//! Proporciona funciones de comparación case-insensitive y ordenamiento natural
//! usando APIs de Windows.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::{fs, io};

use windows::Win32::Foundation::BOOL;
use windows::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
use windows::Win32::UI::Shell::StrCmpLogicalW;
use windows::core::PCWSTR;

/// Compara dos rutas de forma case-insensitive usando CompareStringOrdinal de Windows.
/// 
/// Canonicaliza ambas rutas antes de comparar.
pub fn compare_paths_ignore_case(left: &Path, right: &Path) -> io::Result<bool> {
    let left = fs::canonicalize(left)?;
    let right = fs::canonicalize(right)?;
    let left_wide = left.as_os_str().encode_wide().collect::<Vec<_>>();
    let right_wide = right.as_os_str().encode_wide().collect::<Vec<_>>();

    Ok(unsafe { CompareStringOrdinal(&left_wide, &right_wide, BOOL(1)) == CSTR_EQUAL })
}

/// Compara dos nombres de archivo usando ordenamiento natural de Windows (StrCmpLogicalW).
/// 
/// Este es el mismo ordenamiento que usa el Explorador de Windows.
pub fn compare_names_natural(left: &OsStr, right: &OsStr) -> std::cmp::Ordering {
    let left_wide = left.encode_wide().chain([0]).collect::<Vec<_>>();
    let right_wide = right.encode_wide().chain([0]).collect::<Vec<_>>();
    let result = unsafe { StrCmpLogicalW(PCWSTR(left_wide.as_ptr()), PCWSTR(right_wide.as_ptr())) };

    match result.cmp(&0) {
        std::cmp::Ordering::Equal => left.cmp(right),
        ordering => ordering,
    }
}

/// Compara dos OsStr de forma case-insensitive.
pub fn os_strs_equal_ignore_case(left: &OsStr, right: &OsStr) -> bool {
    let left_wide = left.encode_wide().collect::<Vec<_>>();
    let right_wide = right.encode_wide().collect::<Vec<_>>();
    unsafe { CompareStringOrdinal(&left_wide, &right_wide, BOOL(1)) == CSTR_EQUAL }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_names_natural_handles_numbers() {
        // StrCmpLogicalW ordena "file2" antes que "file10"
        assert_eq!(
            compare_names_natural(OsStr::new("file2.txt"), OsStr::new("file10.txt")),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_names_natural(OsStr::new("file10.txt"), OsStr::new("file2.txt")),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn os_strs_equal_ignore_case_works() {
        assert!(os_strs_equal_ignore_case(OsStr::new("FILE.TXT"), OsStr::new("file.txt")));
        assert!(os_strs_equal_ignore_case(OsStr::new("Image.JPG"), OsStr::new("image.jpg")));
        assert!(!os_strs_equal_ignore_case(OsStr::new("file1.txt"), OsStr::new("file2.txt")));
    }
}