use std::ffi::OsStr;
use std::path::{ Path, PathBuf };

pub struct MimeType {
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

    pub fn image_mime_type_for_extension(path: &Path) -> Option<String> {
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
}

#[test]
fn supported_image_extensions_are_case_insensitive() {
    for extension in [
        "jpg", "jpeg", "jpe", "jfif", "png", "JPG", "JPEG", "JPE", "JFIF", "PNG", "Jpeg",
    ] {
        let path = PathBuf::from(format!("image.{extension}"));

        assert!(
            MimeType::image_mime_type_for_extension(&path).is_some(),
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
            MimeType::image_mime_type_for_extension(&path),
            Some(expected_mime_type)
        );
    }
}

#[test]
fn unsupported_image_extensions_are_rejected() {
    for path in [Path::new("image.gif"), Path::new("image")] {
        assert_eq!(MimeType::image_mime_type_for_extension(path), None);
    }
}