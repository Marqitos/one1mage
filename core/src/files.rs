//! Manejo de archivos y carpetas de imagen.
//! 
//! Este módulo proporciona las estructuras principales para trabajar con archivos
//! de imagen en el sistema de archivos:
//! 
//! - [`FolderInfo`]: Información sobre una carpeta (ruta, nombre canónico).
//! - [`FileInfo`]: Información sobre un archivo de imagen (metadatos, tipo MIME, carpeta).
//! - [`FolderListing`]: Listado de imágenes en una carpeta con orden opcional del Explorador.
//! 
//! El módulo se integra con el Explorador de Windows a través del submódulo [`shell`]
//! para obtener el orden visual de los archivos tal como los ve el usuario.

use crate::mime_type;
use crate::shell::{compare_names_natural, os_strs_equal_ignore_case};

use mime_type::MimeType;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::{fs, io};
use std::slice::Iter;
use crate::shell::explorer::ShellFolder;

/// Información sobre una carpeta del sistema de archivos.
/// 
/// Contiene la ruta original y un nombre canónico (minúsculas) para comparaciones
/// case-insensitive en Windows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderInfo {
    /// Nombre canónico de la carpeta (ruta absoluta en minúsculas).
    canonical_name: String,
    /// Ruta original de la carpeta.
    path: ShellFolder,
}

/// Información sobre un archivo de imagen.
/// 
/// Incluye metadatos del archivo, tipo MIME esperado basado en la extensión,
/// y referencia a la carpeta contenedora.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileInfo {
    /// Nombre canónico del archivo (minúsculas).
    canonical_name: String,
    /// Tipo MIME esperado basado en la extensión del archivo.
    expected_mime_type: String,
    /// Tamaño del archivo en bytes.
    file_size: u64,
    /// Información de la carpeta contenedora.
    folder: FolderInfo,
    /// Nombre original del archivo (preserva mayúsculas/minúsculas).
    name: OsString,
}

/// Listado de archivos de imagen en una carpeta con orden opcional del Explorador.
/// 
/// Proporciona acceso a los archivos de imagen soportados en una carpeta,
/// opcionalmente ordenados según la vista actual del Explorador de Windows.
pub struct FolderListing {
    /// Información de la carpeta.
    folder: FolderInfo,
    /// Archivos de imagen encontrados en la carpeta.
    files: Vec<FileInfo>,
    /// Indica si se usó el orden del Explorador de Windows.
    used_explorer_order: bool,
    /// Índice del archivo actualmente seleccionado (si aplica).
    current_index: Option<usize>,
}

impl FileInfo {
    /// Crea un `FileInfo` desde una ruta de archivo.
    /// 
    /// Valida que la ruta apunte a un archivo existente y que tenga una extensión
    /// de imagen soportada (.jpg, .jpeg, .jpe, .jfif, .png).
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

    /// Obtiene el nombre canónico del archivo (minúsculas).
    pub fn canonical_name(&self) -> &str {
        self.canonical_name.as_ref()
    }

    /// Obtiene el tipo MIME esperado basado en la extensión.
    pub fn expected_mime_type(&self) -> &str {
        self.expected_mime_type.as_ref()
    }

    /// Obtiene la información de la carpeta contenedora.
    pub fn folder(&self) -> &FolderInfo {
        &self.folder
    }

    /// Obtiene el nombre original del archivo.
    pub fn name(&self) -> &OsStr {
        &self.name
    }

    /// Obtiene el tamaño del archivo en bytes.
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Ordena una lista de archivos según el orden del Explorador y/o orden natural.
    /// 
    /// # Argumentos
    /// * `files` - Lista mutable de archivos a ordenar.
    /// * `order` - Orden opcional desde el Explorador de Windows (nombres de archivo en orden visual).
    /// * `selected_file` - Archivo opcional para encontrar su índice tras el ordenamiento.
    /// 
    /// # Retorna
    /// Índice del `selected_file` en la lista ordenada, o 0 si no se proporciona,
    /// o `None` si la lista está vacía.
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
                .then_with(|| compare_names_natural(&left.name, &right.name))
        });

        if let Some(selected) = selected_file {
            files.iter()
                .position(|item|
                    os_strs_equal_ignore_case(&item.name, &selected.name)
                )
        } else if !files.is_empty() {
            Some(0)
        } else {
            None
        }
    }
}

impl FolderInfo {
    /// Crea un `FolderInfo` desde una ruta de carpeta.
    /// 
    /// Valida que la ruta exista y sea un directorio. Canonicaliza la ruta
    /// y genera un nombre canónico en minúsculas para comparaciones case-insensitive.
    pub fn from_path(path: &Path) -> io::Result<FolderInfo> {
        let metadata = fs::metadata(path)?;
        // Comprueba que la ruta sea una carpeta
        if !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "la ruta no apunta a una carpeta",
            ));
        }
        // Obtiene la ruta canonicalizada para comparaciones
        let canonical_path = fs::canonicalize(path)?;

        // Convierte a string y valida que sea Unicode válido
        let canonical_str = canonical_path
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
            path: ShellFolder::new(path.to_path_buf()),
            canonical_name: canonical_str,
        })
    }

    /// Obtiene un `FileInfo` para un archivo dentro de esta carpeta.
    /// 
    /// Valida que el archivo exista y tenga una extensión de imagen soportada
    /// (.jpg, .jpeg, .jpe, .jfif, .png).
    pub fn get_file(&self, file_name: &OsString) -> io::Result<FileInfo> {
        let path_buf = self.path.value()
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

    /// Obtiene el nombre canónico de la carpeta (minúsculas).
    pub fn canonical_name(&self) -> &str {
        &self.canonical_name.as_ref()
    }

    /// Obtiene la ruta original de la carpeta.
    pub fn path(&self) -> &Path {
        self.path.value()
    }

    /// Obtiene el orden actual de archivos en el Explorador de Windows.
    /// 
    /// Consulta la vista activa del Explorador para obtener el orden visual
    /// de los archivos en esta carpeta.
    /// 
    /// # Retorna
    /// `Some(Vec<OsString>)` con los nombres en orden del Explorador,
    /// o `None` si no se puede determinar (ej. carpeta no abierta en el Explorador).
    fn current_explorer_order(&self) -> Option<Vec<OsString>> {
        self.path.get_explorer_order()
    }

    /// Recopila todos los archivos de imagen soportados en la carpeta.
    /// 
    /// Escanea el directorio y filtra solo archivos con extensiones de imagen
    /// soportadas, creando un `FileInfo` para cada uno.
    /// 
    /// # Retorna
    /// Vector de `FileInfo` para cada imagen encontrada, o error de I/O.
    fn collect_supported_files(&self) -> io::Result<Vec<FileInfo>> {
        let mut files = Vec::new();
        for entry in fs::read_dir(self.path.value())? {
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
    /// Crea un `FolderListing` desde un `FileInfo` existente.
    /// 
    /// Carga todos los archivos de imagen soportados en la misma carpeta,
    /// los ordena según el orden del Explorador (si está disponible) y
    /// establece el índice actual en el archivo proporcionado.
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

    /// Crea un `FolderListing` desde un `FolderInfo`.
    /// 
    /// Carga todos los archivos de imagen soportados en la carpeta,
    /// los ordena según el orden del Explorador (si está disponible) y
    /// establece el índice actual en el primer archivo (si hay alguno).
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

    /// Crea un `FolderListing` desde una ruta (archivo o carpeta).
    /// 
    /// Si la ruta es un archivo, crea el listado para su carpeta contenedora
    /// y selecciona ese archivo. Si es una carpeta, crea el listado para
    /// esa carpeta y selecciona el primer archivo.
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

    /// Obtiene el índice del archivo actualmente seleccionado.
    pub fn current_index(&self) -> &Option<usize> {
        &self.current_index
    }

    /// Obtiene un iterador sobre los archivos de imagen en el listado.
    pub fn files(&self) -> Iter<'_, FileInfo> {
        self.files.iter()
    }

    /// Obtiene la información de la carpeta.
    pub fn folder(&self) -> &FolderInfo {
        &self.folder
    }

    /// Indica si se usó el orden del Explorador de Windows para ordenar.
    pub fn used_explorer_order(&self) -> bool {
        self.used_explorer_order
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;
    use crate::files::{FileInfo, FolderInfo};
    use crate::shell::explorer::ShellFolder;

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
                path: ShellFolder::new(PathBuf::from(folder)),
            }
        }
    }

}