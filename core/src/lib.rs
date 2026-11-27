//! Servicios y componentes principales de One1mage

mod database; // Capa de acceso a la base de datos
mod files; // Lectura de la estructura de archivos
mod mime_type; // Comprobación de MIME type por extensión
mod shell; // Integración con Windows Shell
mod window;
mod display;  // Componente de visualización principal
mod renderer; // Renderizador (GPU/CPU)
mod image_loader; // Cargador de imágenes
mod settings; // Configuración de la aplicación
