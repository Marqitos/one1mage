//! Integración con Windows Explorer para obtener orden de archivos
//!
//! Permite obtener el orden de archivos tal como se muestra en una ventana
//! del Explorador de Windows abierta.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::{fs, io};

use windows::{
    Win32::{
        System::Com::{IDispatch, IServiceProvider},
        System::SystemServices::SFGAO_FILESYSTEM,
        UI::Shell::{
            IFolderView, IShellBrowser, IShellItem, IShellItemArray, IShellView, IShellWindows,
            SID_STopLevelBrowser, SIGDN_FILESYSPATH, SVGIO_ALLVIEW, ShellWindows,
        },
    },
    core::{Interface, VARIANT},
};

use crate::shell::com::{ComApartment, TaskMemWide, create_instance};
use crate::shell::path::compare_paths_ignore_case;

/// Representa una carpeta del sistema de archivos con capacidades de Explorer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellFolder(PathBuf);

impl ShellFolder {
    pub fn new(path_buf: PathBuf) -> Self {
        Self(path_buf)
    }
    /// Crea un ShellFolder desde una ruta.
    pub fn from_path(path: &Path) -> io::Result<Self> {
        let metadata = fs::metadata(path)?;
        if !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "la ruta no apunta a una carpeta",
            ));
        }
        Ok(Self(path.to_path_buf()))
    }

    pub fn value(&self) -> &PathBuf {
        &self.0
    }
    /// Obtiene el orden de archivos desde una ventana del Explorador abierta.
    /// 
    /// Busca una ventana del Explorador que esté mostrando esta carpeta
    /// y devuelve los nombres de archivo en el orden visual actual.
    pub fn get_explorer_order(&self) -> Option<Vec<OsString>> {
        let _apartment = ComApartment::new().ok()?;
        let expected_folder = fs::canonicalize(self.0.as_path()).ok()?;

        let shell_windows: IShellWindows = create_instance(&ShellWindows).ok()?;
            let window_count = unsafe { shell_windows.Count() }.ok()?;

        'outer: for index in 0..window_count {
                let dispatch: IDispatch = unsafe { shell_windows.Item(&VARIANT::from(index)) }.ok()?;
            let provider: IServiceProvider = dispatch.cast().ok()?;
                let browser: IShellBrowser = unsafe { provider.QueryService(&SID_STopLevelBrowser) }.ok()?;
                let shell_view: IShellView = unsafe { browser.QueryActiveShellView() }.ok()?;
            let folder_view: IFolderView = shell_view.cast().ok()?;
                let items: IShellItemArray = unsafe { folder_view.Items(SVGIO_ALLVIEW) }.ok()?;
                let count = unsafe { items.GetCount() }.ok()?;

            let mut item_paths = Vec::new();
            for item_index in 0..count {
                    let item: IShellItem = unsafe { items.GetItemAt(item_index) }.ok()?;
                    let attributes = unsafe { item.GetAttributes(SFGAO_FILESYSTEM) }.ok()?;
                if !attributes.contains(SFGAO_FILESYSTEM) {
                    continue;
                }
                let item_path = shell_item_path(&item).ok()?;
                item_paths.push(item_path);
            }

            let Some(view_folder) = item_paths.iter().find_map(|path| path.parent()) else {
                continue 'outer;
            };
            let same = compare_paths_ignore_case(view_folder, &expected_folder).ok()?;
            if !same {
                continue 'outer;
            }

            return Some(
                item_paths
                    .into_iter()
                    .filter_map(|path| path.file_name().map(OsStr::to_os_string))
                    .collect(),
            );
        }
        None
    }
}

/// Obtiene el orden de archivos del Explorador para una carpeta dada.
/// 
/// Función de conveniencia que crea un ShellFolder y obtiene su orden.
pub fn get_explorer_folder_order(folder_path: &Path) -> Option<Vec<OsString>> {
    ShellFolder::from_path(folder_path).ok()?.get_explorer_order()
}

/// Obtiene la ruta del sistema de archivos de un IShellItem.
fn shell_item_path(item: &IShellItem) -> windows::core::Result<PathBuf> {
    let display_name = TaskMemWide(unsafe { item.GetDisplayName(SIGDN_FILESYSPATH)? });
    let path = unsafe { display_name.0.to_string()? };
    Ok(PathBuf::from(path))
}