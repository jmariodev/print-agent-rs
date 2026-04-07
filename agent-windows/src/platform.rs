use std::option::Option;
use async_trait::async_trait;
use anyhow::Result;
use agent_core::traits::Plataforma;
use crate::printer_win::{listar_impresoras_win, imprimir_win};

pub struct WindowsPlatform;

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

    async fn mostrar_notificacion(&self, titulo: &str, mensaje: &str) -> Result<()> {
        use tauri_winrt_notification::{Duration, Sound, Toast};
        
        let _ = Toast::new(Toast::POWERSHELL_APP_ID)
            .title(titulo)
            .text1(mensaje)
            .sound(Some(Sound::SMS))
            .duration(Duration::Short)
            .show();
            
        Ok(())
    }
}
