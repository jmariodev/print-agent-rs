use anyhow::{Context, Result};
use agent_core::config::{Config, Ambiente};
use agent_core::crypto;
use std::path::Path;

const CONFIG_PATH: &str = "config.toml";
const CONFIG_EJEMPLO: &str = r#"
# Agente AIR — Configuración a prueba de tontos
ambiente   = "dev"       # dev | test | prod
id_cliente = "clienteX"
id_punto   = "puntoY"

# VARIABLES AVANZADAS (Puedes dejarlas, o borrarlas y el sistema usará las fijas)
broker_url = "mqtt://127.0.0.1"
broker_port = 1883
update_url = "https://updates.tudominio.com/print-agent/"
log_level  = "info"
"#;

pub fn cargar_config() -> Result<Config> {
    let path = Path::new(CONFIG_PATH);

    if !path.exists() {
        std::fs::write(path, CONFIG_EJEMPLO.trim_start())
            .context("No se pudo crear config.toml de ejemplo")?;
        tracing::error!(
            "No se encontró config.toml. Se creó uno de ejemplo en {:?}. \
             Configúralo y reinicia el servicio.",
            path.canonicalize().unwrap_or(path.to_path_buf())
        );
        std::process::exit(1);
    }

    let contenido = std::fs::read_to_string(path)
        .with_context(|| format!("Error leyendo {}", CONFIG_PATH))?;

    // ── Detectar si el archivo está cifrado o es texto plano ──────────────
    let texto_toml = if crypto::esta_cifrado(&contenido) {
        // Archivo cifrado → descifrar (falla si fue alterado)
        crypto::descifrar(&contenido)
            .context("⚠️ config.toml está cifrado pero no se pudo descifrar. ¿Fue modificado manualmente?")?
    } else {
        // Archivo en texto plano (Dev/Test o primera ejecución en Prod)
        contenido.clone()
    };

    let cfg: Config = toml::from_str(&texto_toml)
        .with_context(|| format!("Error parseando {}", CONFIG_PATH))?;

    // ── Auto-cifrado para Producción ──────────────────────────────────────
    // Si es Prod y el archivo estaba en texto plano, cifrarlo automáticamente
    // para que el técnico no pueda leerlo ni modificarlo a partir de ahora.
    if cfg.ambiente == Ambiente::Prod && !crypto::esta_cifrado(&contenido) {
        match crypto::cifrar(&texto_toml) {
            Ok(cifrado) => {
                if let Err(e) = std::fs::write(path, &cifrado) {
                    tracing::warn!("No se pudo cifrar config.toml para Prod: {}", e);
                } else {
                    tracing::info!("🔒 config.toml cifrado automáticamente para ambiente de Producción.");
                }
            }
            Err(e) => {
                tracing::warn!("Error generando cifrado del config: {}", e);
            }
        }
    }

    Ok(cfg)
}
