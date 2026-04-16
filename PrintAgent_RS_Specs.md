# Agente AIR — Especificaciones Técnicas Completas
> **Archivo de contexto para IA.** Léelo completo antes de escribir cualquier código. Este documento es la fuente de verdad del proyecto.

---

## 1. Visión General

**Agente AIR** es un agente de impresión escrito en Rust que corre como servicio de Windows. Recibe comandos vía MQTT, lista impresoras disponibles y ejecuta trabajos de impresión de PDFs usando SumatraPDF. Incluye auto-actualización segura y logging estructurado.

**Filosofía:** *"Fail fast, recover gracefully"* — el agente **nunca** cuelga en silencio. Todo error se loguea y, si es posible, se notifica vía MQTT.

**Requisito de portabilidad (CRÍTICO):** La arquitectura debe estar diseñada desde el inicio para soportar una futura implementación en Android/iOS. La lógica de negocio en `core/` debe ser completamente agnóstica al sistema operativo. Las implementaciones específicas de plataforma (Win32, JNI/Android, etc.) deben vivir en sus propios crates separados.

---

## 2. Estructura del Workspace

```
print-agent-rs/
├── Cargo.toml                  # [workspace] members = ["core", "agent-windows"]
├── core/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── config.rs           # Tipos fuertes de configuración
│       ├── traits.rs           # Trait `Plataforma` (contrato OS-agnóstico)
│       ├── messages.rs         # Structs serde para JSON MQTT
│       ├── mqtt.rs             # Bucle de eventos MQTT
│       └── updater.rs          # Lógica de auto-actualización
└── agent-windows/
    ├── Cargo.toml
    └── src/
        ├── main.rs             # Punto de entrada, setup logging, servicio Windows
        ├── config_loader.rs    # Lectura de config.toml desde disco
        ├── printer_win.rs      # FFI Win32 para listar impresoras y llamar SumatraPDF
        └── platform.rs         # Struct `WindowsPlatform` que implementa `Plataforma`
```

**Regla de dependencias:**
- `core` NO depende de `agent-windows`.
- `agent-windows` depende de `core`.
- `core` no tiene código `unsafe` ni imports de Win32.

---

## 3. Dependencias (Cargo.toml)

### core/Cargo.toml
```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
rumqttc = "0.24"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
anyhow = "1"
tracing = "0.1"
reqwest = { version = "0.12", features = ["stream"] }
sha2 = "0.10"
base64 = "0.22"
toml = "0.8"
```

### agent-windows/Cargo.toml
```toml
[dependencies]
core = { path = "../core" }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-appender = "0.2"
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_Graphics_Printing",
    "Win32_System_Com",
] }
windows-service = "0.7"
```

---

## 4. Tipos de Datos Centrales

### 4.1 `core/src/config.rs`

```rust
use std::str::FromStr;
use serde::Deserialize;
use anyhow::anyhow;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ambiente {
    Dev,
    Test,
    Prod,
}

impl FromStr for Ambiente {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "dev"  => Ok(Ambiente::Dev),
            "test" => Ok(Ambiente::Test),
            "prod" => Ok(Ambiente::Prod),
            other  => Err(anyhow!("Ambiente inválido: '{}'. Usar dev|test|prod", other)),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub broker_url: String,       // ej: "mqtt://192.168.1.100:1883"
    pub client_id_mqtt: String,   // ej: "sucursal-norte-01"
    pub ambiente: Ambiente,
    pub update_url: String,       // ej: "https://updates.miempresa.com/print-agent/"
    pub log_level: Option<String>, // "debug" | "info" | "warn" — default "info"
}

#[derive(Debug, Clone)]
pub struct Topics {
    pub comandos: String,   // "{client_id}/comandos"
    pub respuestas: String, // "{client_id}/respuestas"
    pub estado: String,     // "{client_id}/estado"
}

impl Topics {
    pub fn from_config(cfg: &Config) -> Self {
        let id = &cfg.client_id_mqtt;
        Topics {
            comandos:   format!("{}/comandos", id),
            respuestas: format!("{}/respuestas", id),
            estado:     format!("{}/estado", id),
        }
    }
}
```

### 4.2 `core/src/messages.rs`

```rust
use serde::{Deserialize, Serialize};

// ── Mensajes ENTRANTES (app móvil → agente) ──────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum Comando {
    ListPrinters,
    PrintPdf(PrintPdfPayload),
    GetStatus,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintPdfPayload {
    pub printer_name: String,
    pub pdf_base64: String,
    pub job_id: String,
}

// ── Mensajes SALIENTES (agente → app móvil) ──────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPrintersResponse {
    pub status: String,
    pub printers: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintResponse {
    pub status: String,
    pub job_id: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub status: String,   // siempre "error"
    pub message: String,
}

impl ErrorResponse {
    pub fn new(msg: impl Into<String>) -> Self {
        ErrorResponse { status: "error".into(), message: msg.into() }
    }
}
```

### 4.3 `core/src/traits.rs` — El Contrato de Plataforma

```rust
use async_trait::async_trait;
use anyhow::Result;

/// Contrato que TODA implementación de plataforma debe cumplir.
/// Diseñado para ser implementado en Windows (Win32), Android (JNI) o cualquier
/// otro sistema operativo en el futuro.
#[async_trait]
pub trait Plataforma: Send + Sync {
    /// Lista los nombres de las impresoras instaladas en el sistema.
    async fn listar_impresoras(&self) -> Result<Vec<String>>;

    /// Envía el PDF en `ruta_pdf` a la impresora `nombre`.
    /// Responsable de invocar el motor de impresión nativo.
    async fn imprimir(&self, nombre: &str, ruta_pdf: &str) -> Result<()>;

    /// Devuelve el nombre de la impresora predeterminada, si existe.
    async fn impresora_predeterminada(&self) -> Result<Option<String>>;
}
```

---

## 5. Implementación Windows

### 5.1 `agent-windows/src/printer_win.rs`

**Listar impresoras con EnumPrintersW:**

```rust
use windows::Win32::Graphics::Printing::{
    EnumPrintersW, PRINTER_ENUM_LOCAL, PRINTER_INFO_2W,
};
use windows::core::PWSTR;
use anyhow::{Result, bail};

pub fn listar_impresoras_win() -> Result<Vec<String>> {
    let mut needed: u32 = 0;
    let mut returned: u32 = 0;

    // Primera llamada: obtener tamaño del buffer
    unsafe {
        let _ = EnumPrintersW(
            PRINTER_ENUM_LOCAL,
            PWSTR::null(),
            2, // nivel PRINTER_INFO_2W
            None,
            &mut needed,
            &mut returned,
        );
    }

    if needed == 0 {
        return Ok(vec![]);
    }

    let mut buf = vec![0u8; needed as usize];

    // Segunda llamada: llenar buffer
    let ok = unsafe {
        EnumPrintersW(
            PRINTER_ENUM_LOCAL,
            PWSTR::null(),
            2,
            Some(&mut buf),
            &mut needed,
            &mut returned,
        )
    };

    if ok.is_err() {
        bail!("EnumPrintersW falló");
    }

    // Aísla el unsafe lo antes posible — trabajar con slices de Rust desde aquí
    let infos: &[PRINTER_INFO_2W] = unsafe {
        std::slice::from_raw_parts(
            buf.as_ptr() as *const PRINTER_INFO_2W,
            returned as usize,
        )
    };

    let nombres = infos
        .iter()
        .filter_map(|info| {
            if info.pPrinterName.is_null() { return None; }
            unsafe { info.pPrinterName.to_string().ok() }
        })
        .collect();

    Ok(nombres)
}
```

**Imprimir con SumatraPDF:**

```rust
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use anyhow::{Result, Context, bail};
use std::path::PathBuf;

/// Guard RAII: elimina el archivo temporal al salir del scope (éxito o error).
pub struct TempPdfGuard {
    pub ruta: PathBuf,
}

impl Drop for TempPdfGuard {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.ruta) {
            tracing::warn!("No se pudo eliminar PDF temporal {:?}: {}", self.ruta, e);
        } else {
            tracing::debug!("PDF temporal eliminado: {:?}", self.ruta);
        }
    }
}

pub async fn imprimir_win(nombre_impresora: &str, ruta_pdf: &str) -> Result<()> {
    // SumatraPDF.exe debe estar en el working directory (C:\PrintAgent\)
    let fut = Command::new("SumatraPDF.exe")
        .args([
            "-print-to", nombre_impresora,
            "-silent",
            ruta_pdf,
        ])
        .status();

    let status = timeout(Duration::from_secs(30), fut)
        .await
        .context("Timeout: SumatraPDF no respondió en 30 segundos")?
        .context("Error al ejecutar SumatraPDF.exe")?;

    if !status.success() {
        bail!("SumatraPDF terminó con código: {:?}", status.code());
    }

    Ok(())
}
```

### 5.2 `agent-windows/src/platform.rs`

```rust
use async_trait::async_trait;
use anyhow::Result;
use core::traits::Plataforma;
use crate::printer_win::{listar_impresoras_win, imprimir_win};

pub struct WindowsPlatform;

#[async_trait]
impl Plataforma for WindowsPlatform {
    async fn listar_impresoras(&self) -> Result<Vec<String>> {
        // Ejecutar en thread pool para no bloquear el runtime de tokio
        tokio::task::spawn_blocking(listar_impresoras_win)
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking falló: {}", e))?
    }

    async fn imprimir(&self, nombre: &str, ruta_pdf: &str) -> Result<()> {
        imprimir_win(nombre, ruta_pdf).await
    }

    async fn impresora_predeterminada(&self) -> Result<Option<String>> {
        // TODO: implementar con GetDefaultPrinterW
        Ok(None)
    }
}
```

---

## 6. Lectura de Configuración

### `agent-windows/src/config_loader.rs`

```rust
use anyhow::{Context, Result};
use core::config::Config;
use std::path::Path;

const CONFIG_PATH: &str = "config.toml";
const CONFIG_EJEMPLO: &str = r#"
# Agente AIR — Configuración
broker_url     = "mqtt://192.168.1.100:1883"
client_id_mqtt = "sucursal-nombre-01"
ambiente       = "prod"   # dev | test | prod
update_url     = "https://updates.tudominio.com/print-agent/"
log_level      = "info"   # debug | info | warn
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

    toml::from_str::<Config>(&texto)
        .with_context(|| format!("Error parseando {}", CONFIG_PATH))
}
```

---

## 7. Bucle de Eventos MQTT

### `core/src/mqtt.rs`

```rust
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS, Event, Incoming};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::watch;
use crate::{config::{Config, Topics}, traits::Plataforma, messages::*};
use base64::{Engine, engine::general_purpose::STANDARD as B64};

pub async fn run(
    cfg: Config,
    topics: Topics,
    plataforma: Arc<dyn Plataforma>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    let opts = MqttOptions::new(&cfg.client_id_mqtt, &cfg.broker_url, 1883);
    let (client, mut event_loop) = AsyncClient::new(opts, 10);

    client.subscribe(&topics.comandos, QoS::AtLeastOnce).await?;

    loop {
        tokio::select! {
            event = event_loop.poll() => {
                match event {
                    Ok(Event::Incoming(Incoming::Publish(p))) => {
                        let plataforma = Arc::clone(&plataforma);
                        let client = client.clone();
                        let topics = topics.clone();

                        tokio::spawn(async move {
                            manejar_mensaje(p.payload.to_vec(), &client, &topics, plataforma).await;
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Error MQTT (reconectando): {}", e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                    _ => {}
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    tracing::info!("Señal de shutdown recibida, cerrando MQTT.");
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn manejar_mensaje(
    payload: Vec<u8>,
    client: &AsyncClient,
    topics: &Topics,
    plataforma: Arc<dyn Plataforma>,
) {
    let respuesta_json = match serde_json::from_slice::<Comando>(&payload) {
        Err(e) => {
            tracing::warn!("JSON inválido recibido: {}", e);
            serde_json::to_string(&ErrorResponse::new(format!("JSON inválido: {}", e)))
                .unwrap_or_default()
        }
        Ok(cmd) => procesar_comando(cmd, plataforma).await,
    };

    if let Err(e) = client
        .publish(&topics.respuestas, QoS::AtLeastOnce, false, respuesta_json.as_bytes())
        .await
    {
        tracing::error!("No se pudo publicar respuesta MQTT: {}", e);
    }
}

async fn procesar_comando(cmd: Comando, plataforma: Arc<dyn Plataforma>) -> String {
    match cmd {
        Comando::ListPrinters => {
            match plataforma.listar_impresoras().await {
                Ok(printers) => serde_json::to_string(&ListPrintersResponse {
                    status: "ok".into(),
                    printers,
                }).unwrap_or_default(),
                Err(e) => serde_json::to_string(&ErrorResponse::new(e.to_string()))
                    .unwrap_or_default(),
            }
        }

        Comando::PrintPdf(payload) => {
            let resultado = imprimir_pdf(payload, plataforma).await;
            match resultado {
                Ok(job_id) => serde_json::to_string(&PrintResponse {
                    status: "ok".into(),
                    job_id,
                    message: "Impresión enviada correctamente".into(),
                }).unwrap_or_default(),
                Err(e) => serde_json::to_string(&ErrorResponse::new(e.to_string()))
                    .unwrap_or_default(),
            }
        }

        Comando::GetStatus => {
            serde_json::json!({ "status": "ok", "agent": "running" }).to_string()
        }
    }
}

async fn imprimir_pdf(
    payload: crate::messages::PrintPdfPayload,
    plataforma: Arc<dyn Plataforma>,
) -> anyhow::Result<String> {
    // Decodificar base64
    let bytes = B64.decode(&payload.pdf_base64)
        .map_err(|e| anyhow::anyhow!("base64 inválido: {}", e))?;

    // Guardar en temp/
    tokio::fs::create_dir_all("temp").await?;
    let ruta = format!("temp/{}.pdf", payload.job_id);
    tokio::fs::write(&ruta, &bytes).await?;

    // Guard RAII: el PDF se borra al salir del scope pase lo que pase
    // NOTA: TempPdfGuard se implementa en agent-windows/src/printer_win.rs
    // En core usamos una closure de limpieza para mantener la agnósticidad de plataforma
    let ruta_clone = ruta.clone();
    let _cleanup = scopeguard::defer(move || {
        let _ = std::fs::remove_file(&ruta_clone);
    });

    plataforma.imprimir(&payload.printer_name, &ruta).await?;

    Ok(payload.job_id)
}
```

---

## 8. Auto-actualización Segura

### `core/src/updater.rs`

```rust
use anyhow::{Result, bail, Context};
use sha2::{Sha256, Digest};
use reqwest::Client;
use tokio::io::AsyncWriteExt;

/// Formato esperado de version.txt en el servidor:
/// ```
/// 1.0.5 a3f8c1d2e4b6...sha256_hash_hex_del_exe
/// ```
pub async fn verificar_y_descargar(update_url: &str, version_actual: &str) -> Result<bool> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let version_url = format!("{}version.txt", update_url);

    let texto = match client.get(&version_url).send().await {
        Err(e) => {
            tracing::warn!("No se pudo verificar actualizaciones (continuando): {}", e);
            return Ok(false);
        }
        Ok(r) => r.text().await?,
    };

    let partes: Vec<&str> = texto.trim().split_whitespace().collect();
    if partes.len() != 2 {
        bail!("version.txt con formato inválido");
    }

    let (version_nueva, hash_esperado) = (partes[0], partes[1]);

    if version_nueva <= version_actual {
        tracing::info!("Agente actualizado (v{}).", version_actual);
        return Ok(false);
    }

    tracing::info!("Nueva versión disponible: {} → {}", version_actual, version_nueva);

    // Descargar en streaming para no cargar el exe completo en RAM
    let exe_url = format!("{}print-agent.exe", update_url);
    let mut respuesta = client.get(&exe_url).send().await
        .context("Error descargando nueva versión")?;

    let tmp_path = "print-agent.new.exe";
    let mut archivo = tokio::fs::File::create(tmp_path).await?;
    let mut hasher = Sha256::new();

    while let Some(chunk) = respuesta.chunk().await? {
        hasher.update(&chunk);
        archivo.write_all(&chunk).await?;
    }
    archivo.flush().await?;

    // Verificar integridad criptográfica
    let hash_calculado = format!("{:x}", hasher.finalize());
    if hash_calculado != hash_esperado {
        tokio::fs::remove_file(tmp_path).await.ok();
        bail!("Hash SHA256 no coincide — posible ataque MITM. Descarga abortada.");
    }

    tracing::info!("Hash verificado. Lanzando actualización...");
    orquestar_reemplazo().await?;

    Ok(true)
}

async fn orquestar_reemplazo() -> Result<()> {
    let bat = r#"@echo off
timeout /t 3 /nobreak > NUL
move /Y "C:\PrintAgent\print-agent.new.exe" "C:\PrintAgent\print-agent.exe"
sc start PrintAgentRS
del "C:\PrintAgent\update.bat"
"#;

    tokio::fs::write("update.bat", bat).await
        .context("No se pudo escribir update.bat")?;

    std::process::Command::new("cmd")
        .args(["/C", "update.bat"])
        .spawn()
        .context("No se pudo lanzar update.bat")?;

    // Liberar el bloqueo sobre el .exe actual
    std::process::exit(0);
}
```

---

## 9. Main y Logging

### `agent-windows/src/main.rs`

```rust
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::watch;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, EnvFilter, prelude::*};

mod config_loader;
mod printer_win;
mod platform;

use platform::WindowsPlatform;
use core::{config::Topics, mqtt};

#[tokio::main]
async fn main() -> Result<()> {
    // ── 1. Fijar working directory al directorio del ejecutable ──────────────
    let exe_path = std::env::current_exe()?;
    std::env::set_current_dir(exe_path.parent().unwrap())?;

    // ── 2. Cargar configuración ──────────────────────────────────────────────
    let cfg = config_loader::cargar_config()?;

    // ── 3. Configurar logging estructurado ───────────────────────────────────
    let level = cfg.log_level.as_deref().unwrap_or("info");
    let file_appender = rolling::daily("logs", "agent.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(EnvFilter::new(level))
        .with(fmt::layer().with_writer(non_blocking))
        .with(fmt::layer().with_writer(std::io::stdout)) // consola también
        .init();

    tracing::info!(
        client_id = %cfg.client_id_mqtt,
        ambiente = ?cfg.ambiente,
        "Agente AIR iniciando..."
    );

    // ── 4. Verificar actualizaciones ─────────────────────────────────────────
    const VERSION_ACTUAL: &str = env!("CARGO_PKG_VERSION");
    if let Err(e) = core::updater::verificar_y_descargar(&cfg.update_url, VERSION_ACTUAL).await {
        tracing::warn!("Error en verificación de actualización: {}", e);
    }

    // ── 5. Iniciar agente ────────────────────────────────────────────────────
    let topics = Topics::from_config(&cfg);
    let plataforma: Arc<dyn core::traits::Plataforma> = Arc::new(WindowsPlatform);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Capturar Ctrl+C / señal de servicio Windows
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Señal de cierre recibida.");
        let _ = shutdown_tx.send(true);
    });

    mqtt::run(cfg, topics, plataforma, shutdown_rx).await?;

    Ok(())
}
```

---

## 10. Compilación y Empaquetado

### Compilación estática (sin dependencias de VC++ Runtime)

```powershell
# En PowerShell, desde la raíz del workspace
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --release --package agent-windows --target x86_64-pc-windows-msvc

# Verificar que solo depende de APIs nativas de Windows
# (requiere Visual Studio Developer Tools)
dumpbin /dependents target\x86_64-pc-windows-msvc\release\print-agent.exe
# Resultado esperado: solo kernel32.dll, ntdll.dll, user32.dll (no msvcrXXX.dll)
```

### Script de empaquetado (PowerShell)

```powershell
# scripts/pack.ps1
$dist = "dist\PrintAgentRS"
New-Item -ItemType Directory -Force $dist | Out-Null

Copy-Item "target\x86_64-pc-windows-msvc\release\print-agent.exe" $dist
Copy-Item "assets\SumatraPDF.exe" $dist
Copy-Item "config.toml.example" $dist

Compress-Archive -Path $dist -DestinationPath "dist\PrintAgentRS.zip" -Force
Write-Host "Distribución generada en dist\PrintAgentRS.zip"
```

### Contenido del ZIP de distribución

```
PrintAgentRS/
├── print-agent.exe        # Binario estático
├── SumatraPDF.exe         # Motor de impresión PDF
└── config.toml.example    # Plantilla de configuración
```

**Instrucciones para el técnico:**
1. Extraer ZIP en `C:\PrintAgent\`
2. Renombrar `config.toml.example` → `config.toml`
3. Editar los 4 campos obligatorios
4. Instalar como servicio: `sc create PrintAgentRS binPath="C:\PrintAgent\print-agent.exe"`
5. Iniciar: `sc start PrintAgentRS`

---

## 11. Protocolo MQTT (Referencia para App Móvil)

### Topics

| Topic                    | Dirección          | Descripción              |
|--------------------------|--------------------|--------------------------|
| `{client_id}/comandos`   | App → Agente       | Comandos de acción       |
| `{client_id}/respuestas` | Agente → App       | Respuestas JSON          |
| `{client_id}/estado`     | Agente → App       | Heartbeat / estado       |

### Ejemplos de mensajes

**Listar impresoras:**
``` json
// Enviar a: {client_id}/comandos
{ "action": "listPrinters" }

// Recibir de: {client_id}/respuestas
{ "status": "ok", "printers": ["HP LaserJet 400", "PDF virtual"] }
```

**Imprimir PDF:**
``` json
// Enviar a: {client_id}/comandos
{
  "action": "printPdf",
  "printerName": "HP LaserJet 400",
  "pdfBase64": "JVBERi0xLjQK...",
  "jobId": "uuid-v4-aqui"
}

// Recibir de: {client_id}/respuestas
{ "status": "ok", "jobId": "uuid-v4-aqui", "message": "Impresión enviada correctamente" }
```

**Error (cualquier comando):**
``` json
{ "status": "error", "message": "Descripción del error" }
```

---

## 12. Hoja de Ruta de Implementación

| Día | Tarea | Crates clave |
|-----|-------|-------------|
| 1 | Scaffold workspace + tipos config + config_loader | `toml`, `serde`, `anyhow` |
| 2 | Trait `Plataforma` + FFI Win32 EnumPrintersW | `windows`, `async-trait` |
| 3 | Conexión MQTT + parseo comandos + listado impresoras | `rumqttc`, `serde_json` |
| 4 | Flujo impresión: base64 → PDF → SumatraPDF + RAII cleanup | `base64`, `tokio::process` |
| 5–6 | Auto-actualización: descarga stream + hash SHA256 + .bat | `reqwest`, `sha2` |
| 7 | Logging rolling + compilación estática + empaquetado ZIP | `tracing-appender` |

---

## 13. Extensión Futura: Android / iOS

La arquitectura está preparada para esto. Los pasos serían:

1. Crear crate `agent-android/` que implemente `Plataforma` usando JNI para llamar al sistema de impresión de Android (Bluetooth, WiFi Direct, etc.).
2. `core/` no necesita cambios — ya es agnóstico al OS.
3. El runtime MQTT puede reutilizarse directamente; solo cambia la implementación del trait.
4. Para iOS: crate `agent-ios/` con bindings a AirPrint via FFI a ObjC/Swift.

**Consideraciones async para móvil:**
- El trait usa `async_trait` desde el inicio para anticipar que JNI/Bluetooth pueden requerir contextos asíncronos.
- En Android, usar `tokio` con `#[cfg(target_os = "android")]` para ajustar el runtime si es necesario.

---

## 14. Checklist de Buenas Prácticas

- [ ] `unsafe` solo en `printer_win.rs`, aislado en funciones pequeñas
- [ ] Todos los errores usan `anyhow` + `.context()` con mensajes útiles
- [ ] Archivos temporales siempre protegidos con RAII (Drop o scopeguard)
- [ ] Timeouts en todas las operaciones externas (SumatraPDF, HTTP, MQTT)
- [ ] Hash SHA256 verificado antes de reemplazar cualquier binario
- [ ] `set_current_dir` al inicio de `main()` para resolución de paths
- [ ] `crt-static` en compilación release para zero runtime dependencies
- [ ] JSON estructurado en todas las respuestas (nunca strings planos)
- [ ] Reconexión MQTT manejada sin pánico
- [ ] Log level configurable por campo en `config.toml`
