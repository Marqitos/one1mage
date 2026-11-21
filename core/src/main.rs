use std::{
    env,
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Debug)]
struct ImageFileInfo {
    folder: PathBuf,
    file_name: OsString,
    size_bytes: u64,
    mime_type: &'static str,
}

fn image_mime_type_for_extension(path: &Path) -> Option<&'static str> {
    let extension = path.extension().and_then(OsStr::to_str)?;
    if ["jpg", "jpeg", "jpe", "jfif"]
        .iter()
        .any(|supported| extension.eq_ignore_ascii_case(supported))
    {
        Some("image/jpeg")
    } else if extension.eq_ignore_ascii_case("png") {
        Some("image/png")
    } else {
        None
    }
}

fn inspect_file(path: &Path) -> io::Result<ImageFileInfo> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "la ruta no apunta a un archivo",
        ));
    }
    let mime_type = image_mime_type_for_extension(path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "archivo no compatible: se admiten .jpg, .jpeg, .jpe, .jfif y .png",
        )
    })?;

    let folder = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "falta el nombre del archivo"))?
        .to_os_string();

    Ok(ImageFileInfo {
        folder,
        file_name,
        size_bytes: metadata.len(),
        mime_type,
    })
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os();
    let program = args.next().unwrap_or_else(|| OsString::from("One1mage"));
    let Some(image_path) = args.next() else {
        return Err(format!("uso: {} <ruta-de-imagen>", Path::new(&program).display()).into());
    };

    let path = PathBuf::from(image_path);
    let info = inspect_file(&path)
        .map_err(|error| format!("no se pudo inspeccionar '{}': {error}", path.display()))?;

    println!("Ruta: {:?}", path);
    println!("Carpeta: {:?}", info.folder);
    println!("Nombre del fichero: {:?}", info.file_name);
    println!("Tamaño: {} bytes", info.size_bytes);
    println!("Tipo MIME: {}", info.mime_type);

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
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TemporaryFile(PathBuf);

    impl TemporaryFile {
        fn new(extension: &str) -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            let path = env::temp_dir().join(format!(
                "one1mage-test-{}-{}.{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed),
                extension
            ));
            std::fs::write(&path, b"test").unwrap();
            Self(path)
        }
    }

    impl Drop for TemporaryFile {
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
            ("jpg", "image/jpeg"),
            ("jpeg", "image/jpeg"),
            ("jpe", "image/jpeg"),
            ("jfif", "image/jpeg"),
            ("png", "image/png"),
            ("JpG", "image/jpeg"),
            ("PnG", "image/png"),
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
    fn inspect_file_returns_folder_name_and_size() {
        let path = TemporaryFile::new("png");
        let expected_size = std::fs::metadata(&path.0).unwrap().len();

        let info = inspect_file(&path.0).unwrap();

        assert_eq!(info.folder, env::temp_dir());
        assert_eq!(info.file_name, path.0.file_name().unwrap());
        assert_eq!(info.size_bytes, expected_size);
        assert_eq!(info.mime_type, "image/png");
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
}
