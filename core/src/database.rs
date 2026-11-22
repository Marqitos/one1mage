use crate::{FileInfo, ImageInfo, SHA256};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::path::Path;
use uuid::Uuid;

const IMAGES_TABLE_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS images (
    id TEXT PRIMARY KEY NOT NULL,
    hash TEXT NOT NULL UNIQUE
        CHECK(length(hash) = 64 AND hash NOT GLOB '*[^0-9a-f]*'),
    file_size INTEGER NOT NULL CHECK(file_size >= 0),
    width INTEGER NOT NULL CHECK(width > 0),
    height INTEGER NOT NULL CHECK(height > 0),
    mime_type TEXT NOT NULL CHECK(mime_type IN ('image/jpeg', 'image/png'))
);
";
const IMAGE_FILES_TABLE_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS image_files (
    folder TEXT NOT NULL,
    name TEXT NOT NULL,
    image_id TEXT NOT NULL,
    PRIMARY KEY (folder, name),
    FOREIGN KEY (image_id) REFERENCES images(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_image_files_image_id ON image_files(image_id);
";
const CURRENT_SCHEMA_VERSION: i64 = 2;

fn image_id_from_hash(hash: &SHA256) -> String {
    let name = format!("1mage:sha256:{hash}");
    Uuid::new_v5(&Uuid::NAMESPACE_OID, name.as_bytes()).to_string()
}

fn sqlite_file_size(file_size: u64) -> rusqlite::Result<i64> {
    i64::try_from(file_size)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

pub(super) struct ImageDatabase {
    connection: Connection,
}

impl ImageDatabase {
    pub(super) fn open(path: &Path) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        let mut schema_version: i64 =
            connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if !(0..=CURRENT_SCHEMA_VERSION).contains(&schema_version) {
            return Err(rusqlite::Error::InvalidQuery);
        }

        while schema_version < CURRENT_SCHEMA_VERSION {
            let transaction = connection.unchecked_transaction()?;
            match schema_version {
                0 => transaction.execute_batch(IMAGES_TABLE_SCHEMA)?,
                1 => transaction.execute_batch(IMAGE_FILES_TABLE_SCHEMA)?,
                _ => return Err(rusqlite::Error::InvalidQuery),
            }
            schema_version += 1;
            transaction.pragma_update(None, "user_version", schema_version)?;
            transaction.commit()?;
        }

        Ok(Self { connection })
    }

    pub(super) fn find_by_path(&self, file: &FileInfo) -> rusqlite::Result<Option<ImageInfo>> {
        let file_size = sqlite_file_size(file.file_size)?;
        self.connection
            .query_row(
                "SELECT i.hash, i.width, i.height, i.mime_type
                 FROM image_files AS f
                 JOIN images AS i ON i.id = f.image_id
                 WHERE f.folder = ?1 AND f.name = ?2 AND i.file_size = ?3",
                params![file.database_folder, file.database_name, file_size],
                |row| {
                    Ok(ImageInfo {
                        file: file.clone(),
                        hash: SHA256::new(row.get(0)?).unwrap(),
                        width: row.get(1)?,
                        height: row.get(2)?,
                        mime_type: row.get(3)?,
                    })
                },
            )
            .optional()
    }

    pub(super) fn find_by_hash(
        &self,
        file: &FileInfo,
        hash: &SHA256,
    ) -> rusqlite::Result<Option<ImageInfo>> {
        let file_size = sqlite_file_size(file.file_size)?;
        self.connection
            .query_row(
                "SELECT width, height, mime_type
                 FROM images
                 WHERE hash = ?1 AND file_size = ?2",
                params![hash.value(), file_size],
                |row| {
                    Ok(ImageInfo {
                        file: file.clone(),
                        hash: hash.clone(),
                        width: row.get(0)?,
                        height: row.get(1)?,
                        mime_type: row.get(2)?,
                    })
                },
            )
            .optional()
    }

    pub(super) fn add_file_location(&self, image: &ImageInfo) -> rusqlite::Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        Self::write_file_location(&transaction, image)?;
        transaction.commit()
    }

    pub(super) fn store_image_file(&self, image: &ImageInfo) -> rusqlite::Result<()> {
        let file_size = sqlite_file_size(image.file.file_size)?;
        let id = image_id_from_hash(&image.hash);
        let transaction = self.connection.unchecked_transaction()?;

        transaction.execute(
            "INSERT INTO images (id, hash, file_size, width, height, mime_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(hash) DO NOTHING",
            params![
                id,
                image.hash.value(),
                file_size,
                i64::from(image.width),
                i64::from(image.height),
                image.mime_type
            ],
        )?;
        Self::write_file_location(&transaction, image)?;
        transaction.commit()
    }

    fn write_file_location(
        transaction: &Transaction<'_>,
        image: &ImageInfo,
    ) -> rusqlite::Result<()> {
        let id = image_id_from_hash(&image.hash);
        transaction.execute(
            "INSERT INTO image_files (folder, name, image_id)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(folder, name) DO UPDATE SET image_id = excluded.image_id",
            params![image.file.database_folder, image.file.database_name, id],
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        ffi::OsString,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct TemporaryDatabase(PathBuf);

    impl TemporaryDatabase {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "one1mage-database-test-{}-{}.sqlite",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            Self(path)
        }
    }

    impl Drop for TemporaryDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn sample_file(folder: &str, name: &str) -> FileInfo {
        FileInfo {
            folder: PathBuf::from(folder),
            file_name: OsString::from(name),
            database_folder: folder.to_owned(),
            database_name: name.to_owned(),
            file_size: 4,
            expected_mime_type: "image/png".to_string(),
        }
    }

    fn sample_image(file: FileInfo, hash: &SHA256) -> ImageInfo {
        ImageInfo {
            file,
            hash: hash.to_owned(),
            width: 2,
            height: 3,
            mime_type: "image/png".to_owned(),
        }
    }

    #[test]
    fn open_creates_the_image_tables_and_enables_foreign_keys() {
        let temp_database = TemporaryDatabase::new();
        let database = ImageDatabase::open(&temp_database.0).unwrap();
        let image_tables: i64 = database
            .connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN ('images', 'image_files')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let foreign_keys_enabled: i64 = database
            .connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        let schema_version: i64 = database
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();

        assert_eq!(image_tables, 2);
        assert!(temp_database.0.exists());
        assert_eq!(foreign_keys_enabled, 1);
        assert_eq!(schema_version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn opening_a_version_one_database_migrates_without_losing_images() {
        let temp_database = TemporaryDatabase::new();
        {
            let connection = Connection::open(&temp_database.0).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE images (
                        id TEXT PRIMARY KEY NOT NULL,
                        hash TEXT NOT NULL UNIQUE,
                        file_size INTEGER NOT NULL,
                        width INTEGER NOT NULL,
                        height INTEGER NOT NULL,
                        mime_type TEXT NOT NULL
                    );
                    INSERT INTO images
                        (id, hash, file_size, width, height, mime_type)
                    VALUES
                        ('existing-id', 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                         5, 2, 3, 'image/png');
                    PRAGMA user_version = 1;",
                )
                .unwrap();
        }

        let database = ImageDatabase::open(&temp_database.0).unwrap();
        let image_count: i64 = database
            .connection
            .query_row("SELECT COUNT(*) FROM images", [], |row| row.get(0))
            .unwrap();
        let schema_version: i64 = database
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();

        assert_eq!(image_count, 1);
        assert_eq!(schema_version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn image_files_reject_unknown_image_ids() {
        let temp_database = TemporaryDatabase::new();
        let database = ImageDatabase::open(&temp_database.0).unwrap();

        let error = database
            .connection
            .execute(
                "INSERT INTO image_files (folder, name, image_id)
                 VALUES (?1, ?2, ?3)",
                params![r"c:\images", "missing.png", "unknown-id"],
            )
            .unwrap_err();

        assert!(matches!(
            error,
            rusqlite::Error::SqliteFailure(ref sqlite_error, _)
                if sqlite_error.code == rusqlite::ErrorCode::ConstraintViolation
        ));
    }

    #[test]
    fn find_by_path_requires_matching_path_and_file_size() {
        let temp_database = TemporaryDatabase::new();
        let database = ImageDatabase::open(&temp_database.0).unwrap();
        let file = sample_file(r"c:\images", "one.png");
        let hash = SHA256::new("a".repeat(64)).unwrap();
        let image = sample_image(file.clone(), &hash);
        database.store_image_file(&image).unwrap();

        let found = database.find_by_path(&file).unwrap().unwrap();
        let mut changed_size = file;
        changed_size.file_size += 1;

        assert_eq!(found.hash, hash);
        assert!(database.find_by_path(&changed_size).unwrap().is_none());
    }

    #[test]
    fn find_by_hash_requires_matching_size_and_links_a_new_path() {
        let temp_database = TemporaryDatabase::new();
        let database = ImageDatabase::open(&temp_database.0).unwrap();
        let hash = SHA256::new("a".repeat(64)).unwrap();
        let original_file = sample_file(r"c:\images", "one.png");
        let original = sample_image(original_file, &hash);
        database.store_image_file(&original).unwrap();

        let duplicate_file = sample_file(r"c:\copies", "copy.png");
        let found = database
            .find_by_hash(&duplicate_file, &hash)
            .unwrap()
            .unwrap();
        database.add_file_location(&found).unwrap();

        let mut wrong_size = duplicate_file.clone();
        wrong_size.file_size += 1;
        assert!(database.find_by_hash(&wrong_size, &hash).unwrap().is_none());
        assert!(database.find_by_path(&duplicate_file).unwrap().is_some());

        let linked_rows: i64 = database
            .connection
            .query_row(
                "SELECT COUNT(*) FROM image_files WHERE image_id = ?1",
                [image_id_from_hash(&hash)],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(linked_rows, 2);
    }

    #[test]
    fn store_image_file_does_not_duplicate_hash_or_path() {
        let temp_database = TemporaryDatabase::new();
        let database = ImageDatabase::open(&temp_database.0).unwrap();
        let hash = SHA256::new("a".repeat(64)).unwrap();
        let first = sample_image(sample_file(r"c:\images", "one.png"), &hash);
        let duplicate_path = sample_image(sample_file(r"c:\images", "one.png"), &hash);
        let duplicate_content = sample_image(sample_file(r"c:\copies", "copy.png"), &hash);

        database.store_image_file(&first).unwrap();
        database.store_image_file(&duplicate_path).unwrap();
        database.store_image_file(&duplicate_content).unwrap();

        let image_rows: i64 = database
            .connection
            .query_row("SELECT COUNT(*) FROM images", [], |row| row.get(0))
            .unwrap();
        let file_rows: i64 = database
            .connection
            .query_row("SELECT COUNT(*) FROM image_files", [], |row| row.get(0))
            .unwrap();

        assert_eq!(image_rows, 1);
        assert_eq!(file_rows, 2);
    }
}
