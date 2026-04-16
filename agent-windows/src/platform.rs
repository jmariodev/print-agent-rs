use std::option::Option;
use async_trait::async_trait;
use anyhow::Result;
use agent_core::traits::Plataforma;
use crate::printer_win::{listar_impresoras_win, imprimir_win};
use winreg::enums::*;
use winreg::RegKey;
use tauri_winrt_notification::{Duration, Sound, Toast};


pub struct WindowsPlatform;

const MI_APP_ID: &str = "Gamasoft.AIR";
const ICONO_BYTES: &[u8] = include_bytes!("../../resources/icon/logo.ico");

pub fn registrar_app_notificaciones() -> Result<()> {
    // Extraer el icono junto al exe en tiempo de ejecución
    let ruta_ico = std::env::current_exe()?
        .parent()
        .unwrap()
        .join("logo.ico");

    // Solo escribirlo si no existe ya
    if !ruta_ico.exists() {
        std::fs::write(&ruta_ico, ICONO_BYTES)?;
    }

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    
    let path_app = format!("Software\\Classes\\AppUserModelId\\{}", MI_APP_ID);
    let (key, _) = hkcu.create_subkey(&path_app)?;
    key.set_value("DisplayName", &"GamaSoft")?;
    key.set_value("IconUri", &ruta_ico.to_string_lossy().as_ref())?;

    let path_notif = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Notifications\\Settings\\{}",
        MI_APP_ID
    );
    let (key2, _) = hkcu.create_subkey(&path_notif)?;
    key2.set_value("ShowInActionCenter", &0u32)?;

    Ok(())
}

#[async_trait]
impl Plataforma for WindowsPlatform {
    async fn listar_impresoras(&self) -> Result<Vec<String>> {
        // Ahora listar_impresoras_win es una función asíncrona (usa tokio::process::Command)
        listar_impresoras_win().await
    }

    async fn imprimir(&self, nombre: &str, ruta_pdf: &str) -> Result<()> {
        imprimir_win(nombre, ruta_pdf).await
    }

    async fn impresora_predeterminada(&self) -> Result<Option<String>> {
        // En el futuro se podría usar "wmic printer where default=true get name"
        Ok(None)
    }

    /* async fn mostrar_notificacion(&self, titulo: &str, mensaje: &str) -> Result<()> {
        use tauri_winrt_notification::{Duration, Sound, Toast};
        
        let _ = Toast::new(Toast::POWERSHELL_APP_ID)
            .title(titulo)
            .text1(mensaje)
            .sound(Some(Sound::SMS))
            .duration(Duration::Short)
            .show();
            
        Ok(())
    } */

    // dentro del impl Plataforma
    async fn mostrar_notificacion(&self, titulo: &str, mensaje: &str) -> Result<()> {
        Toast::new(MI_APP_ID)
            .title(titulo)
            .text1(mensaje)
            .sound(Some(Sound::SMS))
            .duration(Duration::Short)
            .show()?;
        Ok(())
    }
}
