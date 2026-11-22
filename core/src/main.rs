mod database;
mod folder_listing;
mod renderer;
mod window;

use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    process::ExitCode,
};
use std::fmt::Display;

#[derive(Clone, Debug)]
struct FileInfo {
    folder: PathBuf,
    file_name: OsString,
    database_folder: String,
    database_name: String,
    file_size: u64,
    expected_mime_type: String,
}

#[derive(Clone, Debug)]
struct ImageInfo {
    file: FileInfo,
    hash: SHA256,
    width: u32,
    height: u32,
    mime_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SHA256(String);

impl SHA256 {
    pub fn new(hash: String) -> io::Result<Self> {
        if hash.len() != 64
            || !hash
            .to_lowercase()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "el hash SHA-256 debe contener 64 caracteres hexadecimales minúsculos",
            ));
        }
        Ok(SHA256(hash.to_lowercase()))
    }
    fn from_file(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 64 * 1024];

        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        Ok(SHA256(format!("{:x}", hasher.finalize())))
    }
    pub fn value(&self) -> &str{
        &self.0
    }
    pub fn first(&self) -> &str{
        &self.0[..2]
    }
    pub fn last(&self) -> &str{
        &self.0[2..]
    }
}

impl Display for SHA256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

const PART_PRODUCT_FOLDER: &str = "One1mage";
const PART_CACHE_FOLDER: &str = "cache";
const PART_DATABASE_FILE: &str = "1mage.sqlite3";
pub struct Cache(PathBuf);

impl Cache {
    fn path(&self) ->PathBuf {
        self.0.clone()
    }
    fn thumbnail_path(&self, hash: &SHA256) -> io::Result<PathBuf> {

        Ok(self.0
            .join(hash.first())
            .join("thumb")
            .join(hash.last()))
        // TODO: Extensión de imágenes en cache
    }
}
pub struct AppStoragePaths {
    app_data_dir: PathBuf,
}
impl AppStoragePaths {
    fn new(local_app_data: &Path) -> AppStoragePaths {
        AppStoragePaths {
            app_data_dir: local_app_data.join(PART_PRODUCT_FOLDER),
        }
    }
    fn database_file(&self) -> PathBuf {
        self.app_data_dir.join(PART_DATABASE_FILE)
    }
    fn cache(&self) -> Cache {
        Cache(self.app_data_dir.join(PART_CACHE_FOLDER))
    }
}

struct MimeType {
    extensions: Vec<String>,
    mime_type: String
}

impl MimeType {
    fn supported() -> Vec<MimeType> {
        vec![
            MimeType {
                extensions:  vec![
                    "jpg".to_string(),
                    "jpeg".to_string(),
                    "jpe".to_string(),
                    "jfif".to_string()],
                mime_type: "image/jpeg".to_string()
            },
            MimeType {
                extensions:  vec![
                    "png".to_string()],
                mime_type: "image/png".to_string()
            },
        ]
    }
}

fn image_mime_type_for_extension(path: &Path) -> Option<String> {
    let option_extension = path.extension()
        .and_then(OsStr::to_str);
    if let Some(extension) = option_extension {
        for mime in MimeType::supported() {
            if mime.extensions
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported)) {

                return Some(mime.mime_type);
            }
        }
    }
    None
}

fn inspect_file(path: &Path) -> io::Result<FileInfo> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "la ruta no apunta a un archivo",
        ));
    }
    let expected_mime_type = image_mime_type_for_extension(path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "archivo no compatible: se admiten .jpg, .jpeg, .jpe, .jfif y .png",
        )
    })?;
    let canonical_path = std::fs::canonicalize(path)?;

    let folder = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "falta el nombre del archivo"))?
        .to_os_string();
    let database_folder = canonical_path
        .parent()
        .and_then(Path::to_str)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "la carpeta no tiene una ruta Unicode",
            )
        })?
        .to_lowercase();
    let database_name = canonical_path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "el nombre no es Unicode válido")
        })?
        .to_lowercase();

    Ok(FileInfo {
        folder,
        file_name,
        database_folder,
        database_name,
        file_size: metadata.len(),
        expected_mime_type,
    })
}

fn read_image_info(path: &Path, file: FileInfo, hash: SHA256) -> io::Result<ImageInfo> {
    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let actual_mime_type = match reader.format() {
        Some(image::ImageFormat::Jpeg) => "image/jpeg",
        Some(image::ImageFormat::Png) => "image/png",
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "el contenido no es una imagen JPEG o PNG válida",
            ));
        }
    };
    if actual_mime_type != file.expected_mime_type {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "el formato del contenido no coincide con la extensión del archivo",
        ));
    }
    let (width, height) = reader
        .into_dimensions()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;

    Ok(ImageInfo {
        file,
        hash,
        width,
        height,
        mime_type: actual_mime_type.to_owned(),
    })
}

fn validate_cached_mime(file: &FileInfo, mime_type: &str) -> io::Result<()> {
    if mime_type == file.expected_mime_type {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "el formato MIME almacenado no coincide con la extensión del archivo",
        ))
    }
}

fn resolve_image_info(
    database: &database::ImageDatabase,
    path: &Path,
    file: FileInfo,
) -> Result<ImageInfo, Box<dyn std::error::Error>> {
    if let Some(image) = database.find_by_path(&file)? {
        validate_cached_mime(&file, &image.mime_type)?;
        return Ok(image);
    }

    let hash = SHA256::from_file(path)?;
    if let Some(image) = database.find_by_hash(&file, &hash)? {
        validate_cached_mime(&file, &image.mime_type)?;
        database.add_file_location(&image)?;
        return Ok(image);
    }

    let image = read_image_info(path, file, hash)?;
    database.store_image_file(&image)?;
    Ok(image)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os();
    let program = args.next().unwrap_or_else(|| OsString::from("One1mage"));
    let Some(image_path) = args.next() else {
        return Err(format!("uso: {} <ruta-de-imagen>", Path::new(&program).display()).into());
    };

    let path = PathBuf::from(image_path);
    let file = inspect_file(&path)
        .map_err(|error| format!("no se pudo inspeccionar '{}': {error}", path.display()))?;
    let local_app_data = env::var_os("LOCALAPPDATA")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or("no se encontró la carpeta local de datos de Windows (LOCALAPPDATA)")?;
    let storage_paths =  AppStoragePaths::new(&local_app_data);
    std::fs::create_dir_all(
        storage_paths
            .database_file()
            .parent()
            .ok_or("no se pudo determinar la carpeta de la base de datos")?,
    )?;
    let database = database::ImageDatabase::open(&storage_paths.database_file())?;
    let info = resolve_image_info(&database, &path, file)?;
    let folder_images =
        folder_listing::list_supported_images(&info.file.folder, &info.file.database_name)?;
    let thumbnail_path = storage_paths.cache().thumbnail_path(&info.hash)?;
    std::fs::create_dir_all(
        thumbnail_path
            .parent()
            .ok_or("no se pudo determinar la carpeta de la miniatura")?,
    )?;
    println!("Ruta: {:?}", path);
    println!("Carpeta: {:?}", info.file.folder);
    println!("Nombre del fichero: {:?}", info.file.file_name);
    println!("Tamaño: {} bytes", info.file.file_size);
    println!("SHA-256: {}", &info.hash);
    println!("Dimensiones: {}x{}", info.width, info.height);
    println!("Tipo MIME: {}", info.mime_type);
    println!("Base de datos: {:?}", storage_paths.database_file());
    println!("Caché: {:?}", storage_paths.cache().path());
    println!("Ruta de miniatura: {:?}", thumbnail_path);
    println!("Archivos de la carpeta:");
    println!(
        "Orden: {}",
        if folder_images.used_explorer_order {
            "vista abierta del Explorador"
        } else {
            "nombre natural (fallback; no se encontró la vista)"
        }
    );
    for image in folder_images.images {
        let marker = if image.selected { ">" } else { " " };
        println!("{marker} {}", image.name.to_string_lossy());
    }
    window::show(&path, &info.file.file_name.to_string_lossy())?;

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_FILE_ID: AtomicU64 = AtomicU64::new(0);

    struct TemporaryFile(PathBuf);

    impl TemporaryFile {
        fn new(extension: &str) -> Self {
            let format = match extension.to_ascii_lowercase().as_str() {
                "jpg" | "jpeg" | "jpe" | "jfif" => ImageFormat::Jpeg,
                "png" => ImageFormat::Png,
                _ => panic!("unsupported test image extension"),
            };
            Self::with_format(extension, format)
        }

        fn with_format(extension: &str, format: ImageFormat) -> Self {
            let path = env::temp_dir().join(format!(
                "one1mage-test-{}-{}.{}",
                std::process::id(),
                NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed),
                extension
            ));
            DynamicImage::new_rgb8(2, 3)
                .save_with_format(&path, format)
                .unwrap();
            Self(path)
        }

        fn with_bytes(extension: &str, contents: &[u8]) -> Self {
            let path = env::temp_dir().join(format!(
                "one1mage-test-{}-{}.{}",
                std::process::id(),
                NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed),
                extension
            ));
            std::fs::write(&path, contents).unwrap();
            Self(path)
        }
    }

    impl Drop for TemporaryFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    struct TemporaryDatabase(PathBuf);

    impl TemporaryDatabase {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "one1mage-resolution-test-{}-{}.sqlite",
                std::process::id(),
                NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed)
            ));
            Self(path)
        }
    }

    impl Drop for TemporaryDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn supported_image_extensions_are_case_insensitive() {
        for extension in [
            "jpg", "jpeg", "jpe", "jfif", "png", "JPG", "JPEG", "JPE", "JFIF", "PNG", "Jpeg",
        ] {
            let path = PathBuf::from(format!("image.{extension}"));

            assert!(
                image_mime_type_for_extension(&path).is_some(),
                "expected .{extension} to be supported"
            );
        }
    }

    #[test]
    fn image_extensions_return_their_mime_type() {
        for (extension, expected_mime_type) in [
            ("jpg", "image/jpeg".to_string()),
            ("jpeg", "image/jpeg".to_string()),
            ("jpe", "image/jpeg".to_string()),
            ("jfif", "image/jpeg".to_string()),
            ("png", "image/png".to_string()),
            ("JpG", "image/jpeg".to_string()),
            ("PnG", "image/png".to_string()),
        ] {
            let path = PathBuf::from(format!("image.{extension}"));

            assert_eq!(
                image_mime_type_for_extension(&path),
                Some(expected_mime_type)
            );
        }
    }

    #[test]
    fn unsupported_image_extensions_are_rejected() {
        for path in [Path::new("image.gif"), Path::new("image")] {
            assert_eq!(image_mime_type_for_extension(path), None);
        }
    }

    #[test]
    fn inspect_file_returns_path_and_file_metadata_without_hashing() {
        let path = TemporaryFile::new("png");
        let expected_size = std::fs::metadata(&path.0).unwrap().len();

        let info = inspect_file(&path.0).unwrap();

        assert_eq!(info.folder, env::temp_dir());
        assert_eq!(info.file_name, path.0.file_name().unwrap());
        let canonical_path = std::fs::canonicalize(&path.0).unwrap();
        assert_eq!(
            info.database_folder,
            canonical_path
                .parent()
                .unwrap()
                .to_str()
                .unwrap()
                .to_lowercase()
        );
        assert_eq!(
            info.database_name,
            canonical_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_lowercase()
        );
        assert_eq!(info.file_size, expected_size);
        assert_eq!(info.expected_mime_type, "image/png");
    }

    #[test]
    fn inspect_file_does_not_read_image_contents() {
        let contents = b"not actually a png";
        let path = TemporaryFile::with_bytes("png", contents);

        let file_info = inspect_file(&path.0).unwrap();

        assert_eq!(file_info.file_size, contents.len() as u64);
    }

    #[test]
    fn read_image_info_rejects_content_that_does_not_match_extension() {
        let path = TemporaryFile::with_format("jpg", ImageFormat::Png);
        let file = inspect_file(&path.0).unwrap();

        let error = read_image_info(&path.0, file, SHA256::new("a".repeat(64)).unwrap()).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_image_info_returns_hash_dimensions_and_detected_mime() {
        let path = TemporaryFile::new("png");
        let file = inspect_file(&path.0).unwrap();
        let hash = SHA256::from_file(&path.0).unwrap();

        let info = read_image_info(&path.0, file, hash.clone()).unwrap();

        assert_eq!(info.hash, hash);
        assert_eq!((info.width, info.height), (2, 3));
        assert_eq!(info.mime_type, "image/png");
    }

    #[test]
    fn resolve_image_info_uses_the_path_and_size_cache_before_reading_contents() {
        let contents = b"changed but same-sized invalid image";
        let path = TemporaryFile::with_bytes("png", contents);
        let file = inspect_file(&path.0).unwrap();
        let cached = ImageInfo {
            file: file.clone(),
            hash: SHA256::new("c".repeat(64)).unwrap(),
            width: 7,
            height: 9,
            mime_type: "image/png".to_owned(),
        };
        let database_path = TemporaryDatabase::new();
        let database = database::ImageDatabase::open(&database_path.0).unwrap();
        database.store_image_file(&cached).unwrap();

        let resolved = resolve_image_info(&database, &path.0, file).unwrap();

        assert_eq!(resolved.hash, cached.hash);
        assert_eq!((resolved.width, resolved.height), (7, 9));
    }

    #[test]
    fn resolve_image_info_reuses_hash_for_duplicate_file_paths() {
        let original = TemporaryFile::new("png");
        let original_bytes = std::fs::read(&original.0).unwrap();
        let duplicate = TemporaryFile::with_bytes("png", &original_bytes);
        let database_path = TemporaryDatabase::new();
        let database = database::ImageDatabase::open(&database_path.0).unwrap();

        let original_info =
            resolve_image_info(&database, &original.0, inspect_file(&original.0).unwrap()).unwrap();
        let duplicate_info =
            resolve_image_info(&database, &duplicate.0, inspect_file(&duplicate.0).unwrap())
                .unwrap();

        assert_eq!(duplicate_info.hash, original_info.hash);
        assert_eq!(duplicate_info.mime_type, original_info.mime_type);
        assert!(
            database
                .find_by_path(&duplicate_info.file)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn app_storage_paths_use_a_per_user_local_directory() {
        let paths = AppStoragePaths::new(Path::new(r"C:\Users\test\AppData\Local"));

        assert_eq!(
            paths.database_file(),
            Path::new(r"C:\Users\test\AppData\Local\One1mage\1mage.sqlite3")
        );
        assert_eq!(
            paths.cache().path(),
            Path::new(r"C:\Users\test\AppData\Local\One1mage\cache")
        );
    }

    #[test]
    fn inspect_file_rejects_an_unsupported_extension() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");

        let error = inspect_file(&path).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("archivo no compatible"));
    }

    #[test]
    fn inspect_file_rejects_a_directory() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"));

        let error = inspect_file(path).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn inspect_file_reports_a_missing_file() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("this-file-does-not-exist-one1mage-test");

        let error = inspect_file(&path).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn thumbnail_cache_path_uses_hash_sharding() {
        let paths = AppStoragePaths::new(Path::new(r"C:\Users\test\AppData\Local"));
        let hash = SHA256::new("ab".to_owned() + &"c".repeat(62)).unwrap();

        assert_eq!(
            paths.cache().thumbnail_path(&hash).unwrap(),
            Path::new(r"C:\Users\test\AppData\Local\One1mage\cache\ab\thumb").join("c".repeat(62))
        );
    }
}
