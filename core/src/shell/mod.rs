//! Módulo Shell - Integración con Windows Shell y COM
//!
//! Proporciona abstracciones seguras para interactuar con la API de Windows Shell,
//! incluyendo enumeración de ventanas del Explorador, orden de archivos y rutas.

pub mod com;
pub mod explorer;
pub mod path;

pub use explorer::get_explorer_folder_order;
pub use path::{compare_names_natural, os_strs_equal_ignore_case};