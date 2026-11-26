use crate::mime_type;

use mime_type::MimeType;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::{fs, io};
use std::os::windows::ffi::OsStrExt;
use std::slice::Iter;
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

struct ComApartment;

struct TaskMemWide(PWSTR);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderInfo {
    canonical_name: String,
    path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileInfo {
    canonical_name: String,
    expected_mime_type: String,
    file_size: u64,
    folder: FolderInfo,
    name: OsString,
}

pub struct FolderListing {
    folder: FolderInfo,
    // Lista de imágenes cargadas
    // Lista de previsualizaciones
    // Lista de datos sobre imágenes
    // Lista de miniaturas
    // Estado de cada imagen
    files: Vec<FileInfo>,
    used_explorer_order: bool,
    current_index: Option<usize>,
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

impl Drop for TaskMemWide {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFree(Some(self.0.0.cast()));
        }
    }
}

impl FileInfo {
    pub fn from_path(path: &Path) -> io::Result<FileInfo> {
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "la ruta no apunta a un archivo",
            ));
        }
        let folder = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap();
        let folder_info = FolderInfo::from_path(folder)?;
        let file_name: OsString = path.file_name().unwrap()
            .to_os_string();
        folder_info.get_file(&file_name)
    }
    pub fn canonical_name(&self) -> &str {
        &self.canonical_name.as_ref()
    }
    pub fn expected_mime_type(&self) -> &str {
        self.expected_mime_type.as_ref()
    }
    pub fn folder(&self) -> &FolderInfo {
        &self.folder
    }
    pub fn name(&self) -> &OsStr {
        &self.name
    }
    pub fn file_size(&self) -> u64 {
        self.file_size
    }
    pub fn sort(
        files: &mut [FileInfo],
        order: Option<&[OsString]>,
        selected_file: Option<FileInfo>
    ) -> Option<usize> {
        let positions = order.map(|names| {
            names
                .iter()
                .enumerate()
                .map(|(index, name)| (name.clone(), index))
                .collect::<HashMap<OsString, usize>>()
        });
        files.sort_by(|left, right| {
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

        if let Some(selected) = selected_file {
            files.iter()
                .position(|item|
                    os_strs_equal_ignore_case(&item.name, &selected.name)
                )
        } else if files.len() > 0 {
            Some(0)
        } else {
            None
        }
    }
}

impl FolderInfo {
    pub fn from_path(path: &Path) -> io::Result<FolderInfo> {
        let metadata = fs::metadata(path)?;
        // Comprueba que la ruta sea una carpeta
        if !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "la ruta no apunta a una carpeta",
            ));
        }
        // Obtiene la ruta para buscar igualdades
        let canonical_path = fs::canonicalize(path)?
            .as_path()
            .to_str()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "la carpeta no tiene una ruta Unicode",
                )
            })?
            .to_lowercase();
        // Devuelve un nuevo objeto FolderInfo
        Ok(FolderInfo {
            path: path.to_path_buf(),
            canonical_name: canonical_path,
        })
    }
    pub fn get_file(&self, file_name: &OsString) -> io::Result<FileInfo> {
        let path_buf = self.path
            .join(file_name);
        let path: &Path = path_buf.as_path();
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "la ruta no apunta a un archivo",
            ));
        }
        // Comprobamos que apunte a un archivo soportado
        let expected_mime_type =
            MimeType::image_mime_type_for_extension(path).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "archivo no compatible: se admiten .jpg, .jpeg, .jpe, .jfif y .png",
                )
            })?;
        let canonical_name = fs::canonicalize(path)?
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "el nombre no es Unicode válido")
            })?
            .to_lowercase();
        Ok(FileInfo {
            folder: self.clone(),
            name: file_name.clone(),
            file_size: metadata.len(),
            expected_mime_type,
            canonical_name,
        })
    }
    pub fn canonical_name(&self) -> &str {
        &self.canonical_name.as_ref()
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    fn current_explorer_order(&self) -> Option<Vec<OsString>> {
        match unsafe { CoInitialize(None).ok() } {
            Ok(_) => (),
            Err(_) => return None
        };
        let _apartment = ComApartment;
        let expected_folder = match fs::canonicalize(self.path.as_path()) {
            Ok(path) => path,
            Err(_) => return None
        };
        let shell_windows: IShellWindows =
            match unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) } {
                Ok(shell_windows) => shell_windows,
                Err(_) => return None
            };
        let window_count = match unsafe { shell_windows.Count() } {
            Ok(count) => count,
            Err(_) => return None
        };
        'outer: for index in 0..window_count {
            let dispatch: IDispatch = match unsafe { shell_windows.Item(&VARIANT::from(index)) } {
                Ok(dispatch) => dispatch,
                Err(_) => continue 'outer,
            };
            let provider: IServiceProvider = match dispatch.cast() {
                Ok(provider) => provider,
                Err(_) => continue 'outer, /*
                Err(error) if is_no_interface(&error) => continue,
                Err(error) => return Err(error.into()),
                */
            };
            let browser: IShellBrowser =
                match unsafe { provider.QueryService(&SID_STopLevelBrowser) } {
                    Ok(browser) => browser,
                    Err(_) => continue 'outer, /*
                    Err(error) if is_no_interface(&error) => continue,
                    Err(error) => return Err(error.into()),
                    */
                };
            let shell_view: IShellView = match unsafe { browser.QueryActiveShellView() } {
                Ok(shell_view) => shell_view,
                Err(_) => continue 'outer, /*
                Err(error) if is_no_interface(&error) => continue,
                Err(error) => return Err(error.into()),
                */
            };
            let folder_view: IFolderView = match shell_view.cast() {
                Ok(folder_view) => folder_view,
                Err(_) => continue 'outer, /*
                Err(error) if is_no_interface(&error) => continue,
                Err(error) => return Err(error.into()),
                */
            };
            let items: IShellItemArray = match unsafe { folder_view.Items(SVGIO_ALLVIEW) } {
                Ok(items) => items,
                Err(_) => continue 'outer,
            };
            let count: u32 = match unsafe { items.GetCount() } {
                Ok(count) => count,
                Err(_) => continue 'outer,
            };
            let mut item_paths = Vec::new();
            for item_index in 0..count {
                let item: IShellItem = match unsafe { items.GetItemAt(item_index) } {
                    Ok(item) => item,
                    Err(_) => continue 'outer,
                };
                let attributes = match unsafe { item.GetAttributes(SFGAO_FILESYSTEM) } {
                    Ok(attributes) => attributes,
                    Err(_) => continue 'outer,
                };
                if !attributes.contains(SFGAO_FILESYSTEM) {
                    continue;
                }
                let item_path = match shell_item_path(&item) {
                    Ok(item_path) => item_path,
                    Err(_) => continue 'outer,
                };
                item_paths.push(item_path);
            }

            let Some(view_folder) = item_paths.iter().find_map(|path| path.parent()) else {
                continue 'outer;
            };
            let same = match same_canonical_path(view_folder, &expected_folder) {
                Ok(same) => same,
                Err(_) => continue 'outer,
            };
            if !same {
                continue 'outer;
            }

            return Some(
                item_paths
                    .into_iter()
                    .filter_map(|path| path.file_name().map(OsStr::to_os_string))
                    .collect()
            );
        }
        None
    }
    fn collect_supported_files(&self) -> io::Result<Vec<FileInfo>> {
        let mut files = Vec::new();
        for entry in fs::read_dir(self.path.clone())? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if !metadata.is_file() {
                continue;
            }

            let name = entry.file_name();
            if MimeType::image_mime_type_for_extension(Path::new(&name)).is_none() {
                continue;
            }
            let file = match self.get_file(&name.to_os_string()) {
                Ok(file) => file,
                Err(_) => continue
            };
            files.push(file);
        }

        Ok(files)
    }
}

impl FolderListing {
    pub fn from_file_info(file_info: &FileInfo) -> io::Result<FolderListing> {
        let mut files = file_info.folder.collect_supported_files()?;
        let explorer_order = file_info.folder.current_explorer_order();
        let current_index = FileInfo::sort(&mut files, explorer_order.as_deref(), Some(file_info.clone()));
        Ok(FolderListing {
            folder: file_info.folder.clone(),
            files,
            used_explorer_order: explorer_order.is_some(),
            current_index
        })
    }
    pub fn from_folder_info(folder_info: FolderInfo) -> io::Result<FolderListing> {
        let mut files = folder_info.collect_supported_files()?;
        let explorer_order = folder_info.current_explorer_order();
        let current_index = FileInfo::sort(&mut files, explorer_order.as_deref(), None);
        Ok(FolderListing {
            folder: folder_info,
            files,
            used_explorer_order: explorer_order.is_some(),
            current_index
        })
    }
    pub fn from_path(path: &Path) -> io::Result<FolderListing> {
        let metadata = fs::metadata(path)?;
        let folder_info: FolderInfo;
        if metadata.is_file() {
            let folder = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap();
            folder_info = FolderInfo::from_path(folder)?;
            let file_name: OsString = path.file_name().unwrap()
                .to_os_string();
            let file_info: FileInfo = folder_info.get_file(&file_name)?;
            Self::from_file_info(&file_info)
        } else if metadata.is_dir() {
            let folder = path.to_path_buf();
            folder_info = FolderInfo::from_path(folder.as_path())?;
            Self::from_folder_info(folder_info)
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidInput, "ruta no válida"))
        }
    }
    pub fn current_index(&self) -> &Option<usize> {
        &self.current_index
    }
    pub fn files(&self) -> Iter<'_, FileInfo> {
        self.files.iter()
    }
    pub fn folder(&self) -> &FolderInfo {
        &self.folder
    }
    pub fn used_explorer_order(&self) -> bool {
        self.used_explorer_order
    }

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

/*
fn is_no_interface(error: &windows::core::Error) -> bool {
    error.code().0 == 0x8000_4002_u32 as i32
}
*/

fn os_strs_equal_ignore_case(left: &OsStr, right: &OsStr) -> bool {
    let left_wide = left.encode_wide().collect::<Vec<_>>();
    let right_wide = right.encode_wide().collect::<Vec<_>>();
    unsafe { CompareStringOrdinal(&left_wide, &right_wide, BOOL(1)) == CSTR_EQUAL }
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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;
    use crate::files::{FileInfo, FolderInfo};


    impl FileInfo {
        pub fn sample_file(folder: &str, name: &str) -> FileInfo {
            let folder_info = FolderInfo::sample_folder(folder);
            FileInfo {
                canonical_name: name.to_lowercase(),
                expected_mime_type: "image/png".to_string(),
                file_size: 4,
                folder: folder_info,
                name: OsString::from(name),
            }
        }
        pub fn with_file_size(&self, file_size: u64) -> FileInfo {
            FileInfo {
                canonical_name: self.canonical_name().to_string(),
                expected_mime_type: self.expected_mime_type().to_string(),
                file_size,
                folder: self.folder.clone(),
                name: self.name.clone(),
            }
        }
    }

    impl FolderInfo {
        pub fn sample_folder(folder: &str) -> FolderInfo {
            FolderInfo {
                canonical_name: folder.to_lowercase(),
                path: PathBuf::from(folder),
            }
        }
    }

}