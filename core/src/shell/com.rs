//! Gestión de COM (Component Object Model) para Windows
//!
//! Proporciona inicialización segura de COM y gestión de memoria de tareas.

use windows::{
    Win32::{
        System::Com::{
            CoInitialize, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, CoCreateInstance,
        },
    },
    core::{Interface, PWSTR},
};

/// RAII guard para inicialización de COM en el hilo actual.
/// 
/// Inicializa COM al crearse y lo desinicializa al dropearse.
/// Debe crearse una vez por hilo que use APIs COM.
#[derive(Debug)]
pub struct ComApartment;

impl ComApartment {
    /// Inicializa COM para el hilo actual.
    /// 
    /// # Safety
    /// Debe llamarse solo una vez por hilo. El destructor llama a CoUninitialize.
    pub fn new() -> windows::core::Result<Self> {
        unsafe { CoInitialize(None).ok()? };
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

/// RAII wrapper para memoria asignada por COM (CoTaskMemAlloc).
/// 
/// Libera automáticamente la memoria al dropearse usando CoTaskMemFree.
#[derive(Debug)]
pub struct TaskMemWide(pub PWSTR);

impl Drop for TaskMemWide {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFree(Some(self.0 .0.cast()));
        }
    }
}

/// Crea una instancia COM de forma segura.
/// 
/// # Type Parameters
/// * `T` - La interfaz COM a crear (debe implementar `Interface`)
/// 
/// # Arguments
/// * `clsid` - El CLSID de la clase COM
/// 
/// # Returns
/// Una instancia de la interfaz solicitada o error COM.
pub fn create_instance<T: Interface>(clsid: &windows::core::GUID) -> windows::core::Result<T> {
    unsafe { CoCreateInstance(clsid, None, CLSCTX_ALL) }
}