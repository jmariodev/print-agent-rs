use anyhow::Result;
use std::sync::Arc;
use tokio::sync::watch;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, EnvFilter, prelude::*};

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

    // ── 3. Configurar logging estructurado ───────────────────────────────────
    let level = cfg.log_level.as_deref().unwrap_or("info");
    let file_appender = rolling::daily("logs", "agent.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(EnvFilter::new(level))
        .with(fmt::layer().with_writer(non_blocking))
        .with(fmt::layer().with_writer(std::io::stdout))
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

    // Capturar Ctrl+C
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Señal de cierre recibida.");
        let _ = shutdown_tx.send(true);
    });

    mqtt::run(cfg, plataforma, shutdown_rx).await?;

    Ok(())
}
