use crate::image_mime_type_for_extension;
use std::{
    cmp::Ordering,
    collections::HashMap,
    ffi::{OsStr, OsString},
    fs, io,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        Foundation::BOOL,
        Globalization::{CSTR_EQUAL, CompareStringOrdinal},
        System::Com::{
            CLSCTX_ALL, CoCreateInstance, CoInitialize, CoTaskMemFree, CoUninitialize, IDispatch,
            IServiceProvider,
        },
        System::SystemServices::SFGAO_FILESYSTEM,
        UI::Shell::{
            IFolderView, IShellBrowser, IShellItem, IShellItemArray, IShellView, IShellWindows,
            SID_STopLevelBrowser, SIGDN_FILESYSPATH, SVGIO_ALLVIEW, ShellWindows, StrCmpLogicalW,
        },
    },
    core::{Interface, PCWSTR, PWSTR, VARIANT},
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FolderImage {
    pub(crate) name: OsString,
    pub(crate) selected: bool,
}

pub(crate) struct FolderListing {
    pub(crate) images: Vec<FolderImage>,
    pub(crate) used_explorer_order: bool,
}

pub(crate) fn list_supported_images(
    folder: &Path,
    selected_file_name: &str,
) -> Result<FolderListing, Box<dyn std::error::Error>> {
    let explorer_order = active_explorer_order(folder)?;
    let images = collect_supported_images(folder, selected_file_name, explorer_order.as_deref())?;
    Ok(FolderListing {
        images,
        used_explorer_order: explorer_order.is_some(),
    })
}

fn collect_supported_images(
    folder: &Path,
    selected_file_name: &str,
    explorer_order: Option<&[OsString]>,
) -> io::Result<Vec<FolderImage>> {
    let mut images = Vec::new();
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if !metadata.is_file() {
            continue;
        }

        let name = entry.file_name();
        if image_mime_type_for_extension(Path::new(&name)).is_none() {
            continue;
        }
        let selected = os_strs_equal_ignore_case(&name, OsStr::new(selected_file_name));
        images.push(FolderImage { name, selected });
    }

    sort_images(&mut images, explorer_order);
    Ok(images)
}

fn sort_images(images: &mut [FolderImage], explorer_order: Option<&[OsString]>) {
    let positions = explorer_order.map(|names| {
        names
            .iter()
            .enumerate()
            .map(|(index, name)| (name.clone(), index))
            .collect::<HashMap<_, _>>()
    });

    images.sort_by(|left, right| {
        match positions
            .as_ref()
            .map(|positions| (positions.get(&left.name), positions.get(&right.name)))
        {
            Some((Some(left), Some(right))) => left.cmp(right),
            Some((Some(_), None)) => Ordering::Less,
            Some((None, Some(_))) => Ordering::Greater,
            Some((None, None)) | None => Ordering::Equal,
        }
        .then_with(|| compare_names(&left.name, &right.name))
    });
}

fn compare_names(left: &OsStr, right: &OsStr) -> Ordering {
    let left_wide = left.encode_wide().chain([0]).collect::<Vec<_>>();
    let right_wide = right.encode_wide().chain([0]).collect::<Vec<_>>();
    let result = unsafe { StrCmpLogicalW(PCWSTR(left_wide.as_ptr()), PCWSTR(right_wide.as_ptr())) };

    match result.cmp(&0) {
        Ordering::Equal => left.cmp(right),
        ordering => ordering,
    }
}

fn os_strs_equal_ignore_case(left: &OsStr, right: &OsStr) -> bool {
    let left_wide = left.encode_wide().collect::<Vec<_>>();
    let right_wide = right.encode_wide().collect::<Vec<_>>();
    unsafe { CompareStringOrdinal(&left_wide, &right_wide, BOOL(1)) == CSTR_EQUAL }
}

fn active_explorer_order(
    folder: &Path,
) -> Result<Option<Vec<OsString>>, Box<dyn std::error::Error>> {
    unsafe {
        CoInitialize(None).ok()?;
    }
    let _apartment = ComApartment;
    let expected_folder = fs::canonicalize(folder)?;
    let shell_windows: IShellWindows =
        unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)? };

    let window_count = unsafe { shell_windows.Count()? };
    for index in 0..window_count {
        let dispatch: IDispatch = unsafe { shell_windows.Item(&VARIANT::from(index))? };
        let provider: IServiceProvider = match dispatch.cast() {
            Ok(provider) => provider,
            Err(error) if is_no_interface(&error) => continue,
            Err(error) => return Err(error.into()),
        };
        let browser: IShellBrowser = match unsafe { provider.QueryService(&SID_STopLevelBrowser) } {
            Ok(browser) => browser,
            Err(error) if is_no_interface(&error) => continue,
            Err(error) => return Err(error.into()),
        };
        let shell_view: IShellView = match unsafe { browser.QueryActiveShellView() } {
            Ok(shell_view) => shell_view,
            Err(error) if is_no_interface(&error) => continue,
            Err(error) => return Err(error.into()),
        };
        let folder_view: IFolderView = match shell_view.cast() {
            Ok(folder_view) => folder_view,
            Err(error) if is_no_interface(&error) => continue,
            Err(error) => return Err(error.into()),
        };
        let items: IShellItemArray = unsafe { folder_view.Items(SVGIO_ALLVIEW)? };
        let mut item_paths = Vec::new();
        for item_index in 0..unsafe { items.GetCount()? } {
            let item: IShellItem = unsafe { items.GetItemAt(item_index)? };
            if !unsafe { item.GetAttributes(SFGAO_FILESYSTEM)? }.contains(SFGAO_FILESYSTEM) {
                continue;
            }
            item_paths.push(shell_item_path(&item)?);
        }

        let Some(view_folder) = item_paths.iter().find_map(|path| path.parent()) else {
            continue;
        };
        if !same_canonical_path(view_folder, &expected_folder)? {
            continue;
        }

        return Ok(Some(
            item_paths
                .into_iter()
                .filter_map(|path| path.file_name().map(OsStr::to_os_string))
                .collect(),
        ));
    }

    Ok(None)
}

fn is_no_interface(error: &windows::core::Error) -> bool {
    error.code().0 == 0x8000_4002_u32 as i32
}

fn same_canonical_path(left: &Path, right: &Path) -> io::Result<bool> {
    let left = fs::canonicalize(left)?;
    let right = fs::canonicalize(right)?;
    let left_wide = left.as_os_str().encode_wide().collect::<Vec<_>>();
    let right_wide = right.as_os_str().encode_wide().collect::<Vec<_>>();

    Ok(unsafe { CompareStringOrdinal(&left_wide, &right_wide, BOOL(1)) == CSTR_EQUAL })
}

fn shell_item_path(item: &IShellItem) -> windows::core::Result<PathBuf> {
    let display_name = TaskMemWide(unsafe { item.GetDisplayName(SIGDN_FILESYSPATH)? });
    let path = unsafe { display_name.0.to_string()? };
    Ok(PathBuf::from(path))
}

struct TaskMemWide(PWSTR);

impl Drop for TaskMemWide {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFree(Some(self.0.0.cast()));
        }
    }
}

struct ComApartment;

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TemporaryDirectory(PathBuf);

    impl TemporaryDirectory {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "one1mage-folder-list-test-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn lists_only_supported_image_files_in_natural_order_and_marks_selection() {
        let folder = TemporaryDirectory::new();
        for name in ["photo10.png", "photo2.jpg", "readme.txt"] {
            fs::write(folder.0.join(name), b"test").unwrap();
        }
        fs::create_dir(folder.0.join("directory.png")).unwrap();

        let images = collect_supported_images(&folder.0, "photo2.jpg", None).unwrap();

        assert_eq!(
            images,
            [
                FolderImage {
                    name: OsString::from("photo2.jpg"),
                    selected: true,
                },
                FolderImage {
                    name: OsString::from("photo10.png"),
                    selected: false,
                },
            ]
        );
    }

    #[test]
    fn active_explorer_order_is_used_for_visible_images() {
        let folder = TemporaryDirectory::new();
        for name in ["photo10.png", "photo2.jpg", "photo1.png"] {
            fs::write(folder.0.join(name), b"test").unwrap();
        }
        let explorer_order = ["photo10.png", "photo2.jpg"].map(OsString::from);

        let images =
            collect_supported_images(&folder.0, "photo1.png", Some(&explorer_order)).unwrap();

        assert_eq!(
            images
                .iter()
                .map(|image| image.name.clone())
                .collect::<Vec<_>>(),
            [
                OsString::from("photo10.png"),
                OsString::from("photo2.jpg"),
                OsString::from("photo1.png"),
            ]
        );
        assert!(images[2].selected);
    }

    #[test]
    fn selected_file_name_is_matched_with_windows_unicode_case_rules() {
        let folder = TemporaryDirectory::new();
        fs::write(folder.0.join("FotoÉ.png"), b"test").unwrap();

        let images = collect_supported_images(&folder.0, "fotoé.png", None).unwrap();

        assert_eq!(images.len(), 1);
        assert!(images[0].selected);
    }

    #[test]
    fn closed_explorer_folder_uses_natural_name_fallback() {
        let folder = TemporaryDirectory::new();
        fs::write(folder.0.join("photo2.png"), b"test").unwrap();

        let listing = list_supported_images(&folder.0, "photo2.png").unwrap();

        assert!(!listing.used_explorer_order);
        assert_eq!(listing.images.len(), 1);
        assert!(listing.images[0].selected);
    }
}
