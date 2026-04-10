#![windows_subsystem = "windows"]

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::watch;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, EnvFilter, prelude::*};
use agent_core::config::Ambiente;

mod config_loader;
mod printer_win;
mod platform;

use platform::WindowsPlatform;
use agent_core::mqtt;

#[tokio::main]
async fn main() -> Result<()> {
    // ── 1. Fijar working directory al directorio del ejecutable ──────────────
    let exe_path = std::env::current_exe()?;
    if let Some(parent) = exe_path.parent() {
        std::env::set_current_dir(parent)?;
    }

    // ── 2. Cargar configuración ──────────────────────────────────────────────
    let cfg = config_loader::cargar_config()?;

    // ── 2.5 Limpieza de residuos de actualización (OTA) ──────────────────────
    tokio::spawn(async {
        // Darle 5 segundos al Instalador previo (Inno Setup) para que se cierre por completo
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        let _ = tokio::fs::remove_file("PrintAgentRS_Update.exe").await;
        let _ = tokio::fs::remove_file("PrintAgentRS_Installer.tmp.exe").await;
    });

    // ── 3. Configurar logging estructurado ───────────────────────────────────
    let level = cfg.log_level.as_deref().unwrap_or("info");
    let file_appender = rolling::daily("logs", "agent.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(EnvFilter::new(level))
        .with(fmt::layer().with_writer(non_blocking))
        .init();

    tracing::info!(
        client_id = %cfg.client_id_mqtt(),
        ambiente = ?cfg.ambiente,
        "PrintAgent RS iniciando..."
    );

    // ── 4. Verificar actualizaciones ─────────────────────────────────────────
    const VERSION_ACTUAL: &str = env!("CARGO_PKG_VERSION");
    let env_str = format!("{:?}", cfg.ambiente).to_lowercase();
    if let Err(e) = agent_core::updater::verificar_y_descargar(&cfg.update_url_for(&env_str), VERSION_ACTUAL).await {
        tracing::warn!("Error en verificación de actualización: {}", e);
    }

    // ── 5. Iniciar agente ────────────────────────────────────────────────────
    let plataforma: Arc<dyn agent_core::traits::Plataforma> = Arc::new(WindowsPlatform);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Capturar Ctrl+C (por si se corriera atachado a terminal manual)
    let shutdown_tx_ctrlc = shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Señal de cierre recibida vía consola.");
        let _ = shutdown_tx_ctrlc.send(true);
    });

    // ── 6. Canal de pausa (solo funcional en Dev/Test) ────────────────────────
    let (pause_tx, pause_rx) = watch::channel(false);

    // ── 7. Iniciar Bandeja de Sistema (System Tray) ──────────────────────────
    let mut tray = tray_item::TrayItem::new("PrintAgent RS", tray_item::IconSource::Resource("app-icon"))
        .unwrap_or_else(|_| tray_item::TrayItem::new("PrintAgent RS", tray_item::IconSource::Resource("")).unwrap());
    
    let tray_env = format!("PrintAgent: {:?}", cfg.ambiente);
    let _ = tray.add_label(&tray_env);
    
    let _ = tray.inner_mut().add_separator();
    
    // Reiniciar Agente: siempre visible en todos los ambientes
    let shutdown_tx_tray_re = shutdown_tx.clone();
    let _ = tray.add_menu_item("Reiniciar Agente", move || {
        tracing::info!("Usuario solicitó reinicio. Lanzando nueva copia...");
        let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("print-agent.exe"));
        match std::process::Command::new(exe).spawn() {
            Ok(_) => {
                let _ = shutdown_tx_tray_re.send(true);
            }
            Err(e) => {
                tracing::error!("No se pudo reiniciar el Agente: {}", e);
            }
        }
    });

    // Pausar y Cerrar: solo visibles en Dev y Test (en Prod el cliente no debe poder hacerlo)
    if cfg.ambiente == Ambiente::Dev || cfg.ambiente == Ambiente::Test {
        let _ = tray.inner_mut().add_separator();

        let pause_tx_tray = pause_tx.clone();
        let _ = tray.add_menu_item("⏸ Pausar / ▶ Reanudar", move || {
            use tauri_winrt_notification::Toast;

            let actualmente_pausado = *pause_tx_tray.borrow();
            let nuevo_estado = !actualmente_pausado;
            let _ = pause_tx_tray.send(nuevo_estado);
            if nuevo_estado {
                tracing::info!("⏸️  Agente PAUSADO por el usuario. Los mensajes de impresión serán ignorados.");
                let _ = Toast::new(Toast::POWERSHELL_APP_ID)
                    .title("PrintAgent RS")
                    .text1("⏸️ Agente PAUSADO. No procesará impresiones hasta que lo reanudes.")
                    .show();
            } else {
                tracing::info!("▶️  Agente REANUDADO por el usuario.");
                let _ = Toast::new(Toast::POWERSHELL_APP_ID)
                    .title("PrintAgent RS")
                    .text1("▶️ Agente REANUDADO. Procesando impresiones normalmente.")
                    .show();
            }
        });

        let shutdown_tx_tray_cl = shutdown_tx.clone();
        let _ = tray.add_menu_item("Cerrar Agente", move || {
            tracing::info!("Señal de cierre iniciada por usuario desde Bandeja.");
            let _ = shutdown_tx_tray_cl.send(true);
        });
    }
    
    // Mantener la variable tray viva eludiendo el drop hasta que termine main
    let _tray_keeper = tray;

    mqtt::run(cfg, plataforma, shutdown_rx, pause_rx).await?;

    Ok(())
}
