use std::{
    env,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Debug)]
struct ImageFileInfo {
    folder: PathBuf,
    file_name: OsString,
    size_bytes: u64,
}

fn inspect_file(path: &Path) -> io::Result<ImageFileInfo> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "la ruta no apunta a un archivo",
        ));
    }

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

    #[test]
    fn inspect_file_returns_folder_name_and_size() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let expected_size = std::fs::metadata(&path).unwrap().len();

        let info = inspect_file(&path).unwrap();

        assert_eq!(info.folder, Path::new(env!("CARGO_MANIFEST_DIR")));
        assert_eq!(info.file_name, OsString::from("Cargo.toml"));
        assert_eq!(info.size_bytes, expected_size);
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
