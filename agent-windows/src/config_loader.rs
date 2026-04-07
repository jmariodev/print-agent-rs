use anyhow::{Context, Result};
use agent_core::config::Config;
use std::path::Path;

const CONFIG_PATH: &str = "config.toml";
const CONFIG_EJEMPLO: &str = r#"
# PrintAgent RS — Configuración a prueba de tontos
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

    let texto = std::fs::read_to_string(path)
        .with_context(|| format!("Error leyendo {}", CONFIG_PATH))?;

    let cfg: Config = toml::from_str(&texto)
        .with_context(|| format!("Error parseando {}", CONFIG_PATH))?;

    Ok(cfg)
}
