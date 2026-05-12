#![windows_subsystem = "windows"]

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::watch;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, EnvFilter, prelude::*};
use crate::platform::registrar_app_notificaciones;

mod config_loader;
mod printer_win;
mod platform;

use platform::WindowsPlatform;
use agent_core::mqtt;

#[tokio::main]
async fn main() -> Result<()> {
    // ── 0. Registrar notificaciones ──────────────────────────────────────────
    registrar_app_notificaciones().expect("Error registrando notificaciones");

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
        version = %env!("CARGO_PKG_VERSION"),
        client_id = %cfg.client_id_mqtt(),
        "🚀 >>> INICIANDO AGENTE AIR <<<"
    );

    // --- TRAZABILIDAD: Identificar si venimos de una resurrección ---
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--revived") {
        tracing::warn!("🧟 REANIMACIÓN: El Agente ha sido levantado por el Guardián tras un cierre inesperado.");
    }

    // --- PROTECCIÓN MUTUA: Lanzar o asegurar que el Guardián esté vivo ---
    let exe_path = std::env::current_exe().unwrap();
    let exe_dir = exe_path.parent().unwrap();
    let vbs_path = exe_dir.join("lanzador.vbs");
    
    if vbs_path.exists() {
        // --- LIMPIEZA: Borrar candado de parada previo si existe ---
        let stop_lock = exe_dir.join("stop.lock");
        if stop_lock.exists() {
            let _ = std::fs::remove_file(&stop_lock);
            tracing::debug!("🔓 Sesión previa desbloqueada.");
        }

        // Intentamos lanzar el script. Gracias a su candado interno, 
        // si ya está corriendo, la nueva instancia se cerrará sola.
        match std::process::Command::new("wscript.exe")
            .arg(vbs_path.to_str().unwrap_or("lanzador.vbs"))
            .current_dir(exe_dir)
            .spawn() 
        {
            Ok(_) => tracing::info!("🛡️ Protección de Alta Disponibilidad: ACTIVA"),
            Err(e) => tracing::error!("❌ Fallo al activar el Guardián VBScript: {}", e),
        }
    } else {
        tracing::warn!("⚠️ Guardián (lanzador.vbs) no encontrado. La protección está desactivada.");
    }
    // -------------------------------------------------------------------

    // ── 4. Verificar actualizaciones ─────────────────────────────────────────
    const VERSION_ACTUAL: &str = env!("CARGO_PKG_VERSION");
    let env_str = cfg.ambiente.base_env();
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
    let mut tray = tray_item::TrayItem::new("Agente AIR", tray_item::IconSource::Resource("app-icon"))
        .unwrap_or_else(|_| tray_item::TrayItem::new("Agente AIR", tray_item::IconSource::Resource("")).unwrap());
    
    let tray_env = format!("AIR: {}", cfg.client_id_mqtt().to_uppercase());
    let _ = tray.add_label(&tray_env);
    let _ = tray.add_label(&format!("VERSION: {}", VERSION_ACTUAL));
    
    let _ = tray.inner_mut().add_separator();
    
    // Reiniciar Agente: siempre visible en todos los ambientes
    let shutdown_tx_tray_re = shutdown_tx.clone();
    let _ = tray.add_menu_item("Reiniciar Agente", move || {
        tracing::info!("Usuario solicitó reinicio. Cerrando agente para que el guardián lo reviva...");
        let _ = shutdown_tx_tray_re.send(true);
    });

    // Ver Logs: siempre visible, abre la carpeta de logs en Windows Explorer
    let _ = tray.add_menu_item("Ver Logs", move || {
        tracing::info!("Abriendo carpeta de logs...");
        let _ = std::process::Command::new("explorer")
            .arg("logs")
            .spawn();
    });



    // Opciones administrativas: solo visibles en Dev y Test (en Prod el cliente no debe poder hacerlo)
    if cfg.ambiente.is_dev_or_test() {
        let _ = tray.inner_mut().add_separator();

        // Cambiar Configuración: descifra temporalmente si es necesario y abre notepad
        let exe_dir_cfg = exe_path.parent().unwrap().to_path_buf();
        let _ = tray.add_menu_item("Cambiar Configuración", move || {
            tracing::info!("Abriendo archivo de configuración...");
            let config_path = exe_dir_cfg.join("config.toml");
            if let Ok(contenido) = std::fs::read_to_string(&config_path) {
                if agent_core::crypto::esta_cifrado(&contenido) {
                    if let Ok(descifrado) = agent_core::crypto::descifrar(&contenido) {
                        let _ = std::fs::write(&config_path, descifrado);
                    }
                }
            }
            let _ = std::process::Command::new("notepad.exe")
                .arg(config_path.to_str().unwrap_or("config.toml"))
                .spawn();
                
            crate::platform::mostrar_notificacion_local(
                "Modo Edición", 
                "Guarda los cambios en el bloc de notas y haz clic en 'Reiniciar Agente' para aplicar y asegurar la configuración."
            );
        });

        // Desinstalar Agente
        let exe_dir_unins = exe_path.parent().unwrap().to_path_buf();
        let shutdown_tx_tray_unins = shutdown_tx.clone();
        let _ = tray.add_menu_item("Desinstalar Agente", move || {
            tracing::info!("Iniciando desinstalador...");
            let unins_path = exe_dir_unins.join("unins000.exe");
            if unins_path.exists() {
                // Escribir stop.lock para desactivar el guardián y que no nos reviva
                let _ = std::fs::write(exe_dir_unins.join("stop.lock"), "stop");
                let _ = std::process::Command::new(&unins_path).spawn();
                // Cerrar el agente inmediatamente para que el desinstalador 
                // no se suicide al hacer taskkill /T al agente (su padre).
                let _ = shutdown_tx_tray_unins.send(true);
            } else {
                crate::platform::mostrar_notificacion_local(
                    "Error", 
                    "No se encontró el desinstalador (unins000.exe)."
                );
            }
        });

        let _ = tray.inner_mut().add_separator();

        let pause_tx_tray = pause_tx.clone();
        let _ = tray.add_menu_item("⏸ Pausar / ▶ Reanudar", move || {
            let actualmente_pausado = *pause_tx_tray.borrow();
            let nuevo_estado = !actualmente_pausado;
            let _ = pause_tx_tray.send(nuevo_estado);

            if nuevo_estado {
                tracing::info!("⏸️  Agente PAUSADO por el usuario. Los mensajes de impresión serán ignorados.");
                crate::platform::mostrar_notificacion_local("Agente AIR", "⏸️ Agente PAUSADO. No procesará impresiones hasta que lo reanudes.");
            } else {
                tracing::info!("▶️  Agente REANUDADO por el usuario.");
                crate::platform::mostrar_notificacion_local("Agente AIR", "▶️ Agente REANUDADO. Procesando impresiones normalmente.");
            }
        });

        let shutdown_tx_tray_cl = shutdown_tx.clone();
        let exe_dir_cl = exe_path.parent().unwrap().to_path_buf();
        let _ = tray.add_menu_item("Cerrar Agente", move || {
            tracing::info!("Señal de cierre iniciada por usuario desde Bandeja.");
            // Escribir stop.lock para desactivar el guardián permanentemente
            let _ = std::fs::write(exe_dir_cl.join("stop.lock"), "stop");
            let _ = shutdown_tx_tray_cl.send(true);
        });
    }
    
    // Mantener la variable tray viva eludiendo el drop hasta que termine main
    let _tray_keeper = tray;

    mqtt::run(cfg, plataforma, shutdown_rx, pause_rx).await?;

    Ok(())
}
