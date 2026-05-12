# Agente AIR — Especificaciones Técnicas

Referencia técnica completa para desarrolladores. Refleja el estado real del código en producción.

---

## 1. Visión General

**Agente AIR** es un daemon de impresión escrito en Rust que corre como proceso invisible en Windows. Recibe trabajos vía MQTT, decodifica PDFs en Base64, los renderiza con Win32 GDI usando `pdfium-render` y los envía al spooler de impresión. Incluye cifrado de configuración, actualizaciones OTA, alta disponibilidad con guardian VBScript y logging rolling estructurado.

**Filosofía:** La lógica de negocio en `core/` es completamente agnóstica al sistema operativo. Las implementaciones de plataforma viven en crates separados. Esta separación es el pilar para futura expansión a Android/iOS.

---

## 2. Estructura del Workspace

```
print-agent-rs/
├── Cargo.toml                    # workspace, resolver = "2", members = ["core", "agent-windows"]
├── core/                         # Librería cross-platform — sin Win32, sin unsafe
│   ├── Cargo.toml                # version = "26.1.0", edition = "2024"
│   └── src/
│       ├── lib.rs                # Re-exporta todos los módulos públicos
│       ├── config.rs             # Tipos Ambiente (newtype) y Config
│       ├── crypto.rs             # AES-256-GCM para config.toml
│       ├── messages.rs           # Enum Comando (serde, discriminado por "action")
│       ├── mqtt.rs               # Event loop MQTT + dispatcher de comandos
│       ├── traits.rs             # Trait Plataforma (contrato OS-agnóstico)
│       └── updater.rs            # OTA: download stream + SHA-256 + Inno Setup
└── agent-windows/                # Implementación Windows
    ├── Cargo.toml                # version = "26.1.1", edition = "2024"
    ├── build.rs                  # Embed icon en el ejecutable (embed-resource)
    └── src/
        ├── main.rs               # Entry point, tray icon, logging, guardian VBS
        ├── config_loader.rs      # Carga config.toml con auto-cifrado prod
        ├── platform.rs           # WindowsPlatform + notificaciones WinRT
        └── printer_win.rs        # EnumPrintersW + pdfium GDI + ESC/POS RAW
```

**Regla de dependencias:**
- `core` NO depende de `agent-windows`
- `agent-windows` depende de `core` via path dependency
- `core` no contiene código `unsafe` ni imports Win32

---

## 3. Dependencias

### core/Cargo.toml

| Crate | Versión | Propósito |
|---|---|---|
| `tokio` | 1 (full) | Runtime async |
| `rumqttc` | 0.25.1 | Cliente MQTT con WSS/TLS Rustls |
| `serde` + `serde_json` | 1 | Serialización JSON |
| `async-trait` | 0.1 | Métodos async en traits |
| `anyhow` | 1 | Propagación de errores con contexto |
| `tracing` | 0.1 | Logging estructurado |
| `reqwest` | 0.13.2 | Descarga streaming OTA |
| `sha2` | 0.11.0 | SHA-256 para verificación OTA |
| `base64` | 0.22 | Decodificación de PDFs entrantes |
| `toml` | 0.8 | Parseo de config.toml |
| `aes-gcm` | 0.10.3 | Cifrado AES-256-GCM |
| `rustls` | 0.23 | TLS para WSS |
| `scopeguard` | 1.2 | RAII cleanup de archivos temporales |

### agent-windows/Cargo.toml

| Crate | Versión | Propósito |
|---|---|---|
| `agent-core` | path | Dependencia local |
| `tokio` | 1 (full) | Runtime async |
| `pdfium-render` | 0.8 | Rasterizar PDF a bitmap para GDI |
| `windows` | 0.58 | Win32: GDI, Printing, COM, XPS |
| `windows-service` | 0.7 | Ciclo de vida servicio Windows |
| `tray-item` | 0.10 | Ícono en bandeja del sistema |
| `tauri-winrt-notification` | 0.2 | Toast notifications WinRT |
| `winreg` | 0.52 | Registro de Windows (AppUserModelId) |
| `tracing-subscriber` | 0.3 | Configuración de tracing |
| `tracing-appender` | 0.2 | Rolling log diario |
| `embed-resource` | 2.4 | Build dep: embeber icon en .exe |

---

## 4. Tipos de Datos Centrales

### `Ambiente` — `core/src/config.rs`

Newtype validado que acepta `dev`, `test`, `prod`, o cualquier variante `prod_*`.

```rust
pub struct Ambiente(pub String);

impl TryFrom<String> for Ambiente { ... }  // valida en tiempo de parseo
impl FromStr for Ambiente { ... }

impl Ambiente {
    pub fn is_prod(&self) -> bool { self.0.starts_with("prod") }
    pub fn is_dev_or_test(&self) -> bool { self.0 == "dev" || self.0 == "test" }
    pub fn as_str(&self) -> &str { &self.0 }
    pub fn base_env(&self) -> &str {
        // prod_gd4 → "prod", dev → "dev"
        if self.is_prod() { "prod" } else { self.as_str() }
    }
}
```

### `Config` — `core/src/config.rs`

```rust
pub struct Config {
    pub ambiente:    Ambiente,
    pub id_cliente:  String,
    pub id_punto:    String,
    pub broker_url:  Option<String>,   // default: "wss://gd5.gamasoftcol.com"
    pub broker_port: Option<u16>,      // default: 1883
    pub update_url:  Option<String>,   // default: construido desde ambiente
    pub log_level:   Option<String>,   // default: "info"
}

impl Config {
    // Topics MQTT derivados de los campos
    pub fn topic_subscripcion(&self) -> String
        // → "{ambiente}-{id_cliente}-{id_punto}-imp-local"
    pub fn topic_broadcast_update(&self) -> String
        // → "update-air-{base_env}"
    pub fn client_id_mqtt(&self) -> String
        // → "{ambiente}-{id_cliente}-{id_punto}"

    // URL parsing inteligente (WSS vs TCP puro)
    pub fn broker_url(&self) -> String
    pub fn broker_port(&self) -> u16
    pub fn is_wss(&self) -> bool
    pub fn update_url_for(&self, target_env: &str) -> String
}
```

**config.toml de referencia:**
```toml
ambiente    = "prod_gd4"
id_cliente  = "gama"
id_punto    = "001"
broker_url  = "wss://broker.ejemplo.com/mqtt"
broker_port = 8883
update_url  = "https://updates.ejemplo.com/ActualizadorAIR/prod/"
log_level   = "info"
```

### `Comando` — `core/src/messages.rs`

Discriminado por el campo `action` del JSON entrante:

```rust
#[serde(tag = "action")]
pub enum Comando {
    #[serde(rename = "listPrinters")]
    ListPrinters {
        #[serde(rename = "responseTopic")]
        response_topic: String,
    },
    #[serde(rename = "print")]
    Print {
        #[serde(rename = "responseTopic")]
        response_topic: String,
        #[serde(rename = "printerName")]
        printer_name: String,
        #[serde(rename = "fileToPrint")]
        file_to_print: String,  // PDF en Base64
    },
    #[serde(rename = "update-air")]
    UpdateAir {
        #[serde(rename = "responseTopic")]
        response_topic: String,
        ambiente: Option<String>,
    },
}

// Fallback mínimo para extraer responseTopic de JSON malformado
pub struct FallbackComando {
    pub response_topic: Option<String>,  // "responseTopic"
}
```

### Trait `Plataforma` — `core/src/traits.rs`

```rust
#[async_trait]
pub trait Plataforma: Send + Sync {
    async fn listar_impresoras(&self) -> Result<Vec<String>>;
    async fn imprimir(&self, nombre: &str, ruta_pdf: &str) -> Result<()>;
    async fn impresora_predeterminada(&self) -> Result<Option<String>>;
    async fn mostrar_notificacion(&self, titulo: &str, mensaje: &str) -> Result<()>;
}
```

`WindowsPlatform` en `agent-windows/src/platform.rs` implementa este trait. Toda futura plataforma (Android, iOS) debe implementar solo este contrato.

---

## 5. Cifrado de Configuración — `core/src/crypto.rs`

El `config.toml` se cifra automáticamente en ambientes `prod*` al primer arranque.

| Elemento | Valor |
|---|---|
| Algoritmo | AES-256-GCM |
| Clave | `SHA-256(INTERNAL_SECRET)` — secreto embebido en el binario |
| Nonce | 12 bytes aleatorios por cifrado |
| Magic prefix | `PAGENT_ENC:` — identifica archivos cifrados |
| Formato | `PAGENT_ENC:{base64(nonce + ciphertext)}` |

**Flujo de carga (`config_loader.rs`):**
1. Leer `config.toml`
2. Si empieza con `PAGENT_ENC:` → descifrar
3. Parsear TOML → `Config`
4. Si `is_prod()` y el archivo estaba en plaintext → cifrar y reescribir

---

## 6. Event Loop MQTT — `core/src/mqtt.rs`

```rust
pub async fn run(
    cfg: Config,
    plataforma: Arc<dyn Plataforma>,
    mut shutdown_rx: watch::Receiver<bool>,
    pause_rx: watch::Receiver<bool>,
) -> Result<()>
```

**Flujo:**
1. Crear `AsyncClient` con opciones (WSS/TLS o TCP)
2. Límite de payload: 20 MB
3. `loop { tokio::select! { ... } }`:
   - `ConnAck` → suscribir a topic transaccional + broadcast OTA
   - `Publish` → si es broadcast: verificar actualización; si es transaccional y no pausado: `tokio::spawn(manejar_mensaje(...))`
   - Error de conexión → notificar + sleep 2s + rumqttc reconecta automáticamente
   - `shutdown_rx.changed()` → break

**TLS personalizado:** `InsecureCertVerifier` acepta certificados autofirmados del broker. Pendiente migrar a cert pinning (ver `ARQUITECTURA.md`).

**Procesamiento de comandos:**

| Comando | Acción | Respuesta |
|---|---|---|
| `ListPrinters` | `plataforma.listar_impresoras()` | `"[Printer1, Printer2, ...]"` |
| `Print` | Decodifica Base64 → temp PDF → `plataforma.imprimir()` → limpia temp | `"Impresion exitosa"` o `"Error al imprimir"` |
| `UpdateAir` | `tokio::spawn(updater::verificar_y_descargar(...))` | `"Verificando actualizaciones"` |

Los PDFs temporales se escriben en `temp/{timestamp_ms}.pdf` y se eliminan con `scopeguard::defer!` (RAII, incluso ante panics).

---

## 7. Sistema de Impresión Win32 — `agent-windows/src/printer_win.rs`

### Listar impresoras (`EnumPrintersW`)

Patrón de doble llamada Win32:
1. Primera llamada con buffer `None` → obtener tamaño necesario en `cb_needed`
2. Segunda llamada con buffer de `cb_needed` bytes → llenar `PRINTER_INFO_4W[]`
3. Extraer `pPrinterName` de cada struct

Flags: `PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS` (impresoras locales y de red).

### Imprimir PDF (`pdfium + GDI`)

```
Cargar PDF con pdfium-render
  → CreateDCW(nombre_impresora)
  → StartDocW
  → Por cada página:
      render_with_config(target_width = HORZRES)
      → bitmap BGRA
      → flip de filas (Top-Down → Bottom-Up para compatibilidad GDI)
      → StretchDIBits(hdc, ...)
      → StartPage / EndPage
  → EndDoc / DeleteDC
```

El flip de filas es necesario para compatibilidad con el 100% de drivers Windows, incluyendo impresoras antiguas (SAT, Xprinter genéricas).

### Corte de papel ESC/POS

Después de cada impresión GDI:
1. `OpenPrinterW` con `PRINTER_ACCESS_USE`
2. `StartDocPrinterW` con datatype `"RAW"`
3. `WritePrinter` con `[0x1D, 0x56, 0x42, 0x00]` (GS V B — partial cut)
4. `EndDocPrinter` / `ClosePrinter`

### Mutex de impresión

```rust
static PRINT_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
```

Garantiza serialización si el broker MQTT envía múltiples trabajos en ráfaga.

---

## 8. OTA — `core/src/updater.rs`

```
GET {update_url}version.txt
  → Parsear: "{version_nueva} {sha256_hex}"
  → Comparar versiones (string, formato semver)  ⚠ pendiente migrar a comparación numérica
  → Si version_nueva > version_actual:
      GET {update_url}PrintAgentRS_Installer.exe (streaming)
        → Hashear en tiempo real con SHA-256
        → Escribir a PrintAgentRS_Installer.tmp.exe
      drop(archivo)   ← liberar handle antes de ejecutar
      Verificar hash
      rename .tmp.exe → PrintAgentRS_Update.exe
      spawn("PrintAgentRS_Update.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART")
      exit(0)         ← liberar file locks para que Inno Setup sobreescriba
```

**Seguridad:** La descarga aborta si el hash SHA-256 no coincide (protección MITM). El binario no expone información de versión al servidor.

---

## 9. Sistema Tray y Canales — `agent-windows/src/main.rs`

### Canales Tokio

```rust
// Apagado limpio del event loop MQTT
let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

// Pausar/reanudar procesamiento de trabajos (solo dev/test)
let (pause_tx, pause_rx) = tokio::sync::watch::channel(false);
```

### Menú de bandeja

| Ítem | Visible en | Acción |
|---|---|---|
| `AIR: {ambiente}-{cliente}-{punto}` | Siempre | Label informativo |
| `v{version}` | Siempre | Label de versión |
| `Reiniciar Agente` | Siempre | `shutdown_tx.send(true)` — el Guardian revive el proceso |
| `Ver Logs` | Siempre | Abre `logs/` en Explorer |
| `Cambiar Configuración` | Solo dev/test | Descifra → Notepad → re-cifra al reiniciar |
| `Desinstalar Agente` | Solo dev/test | Escribe `stop.lock` → lanza `unins000.exe` |
| `Pausar / Reanudar` | Solo dev/test | Toggle `pause_tx` |
| `Cerrar Agente` | Solo dev/test | Escribe `stop.lock` → `shutdown_tx.send(true)` |

La distinción dev/test vs producción se evalúa con `cfg.ambiente.is_dev_or_test()`.

### Guardian VBScript

El instalador genera `lanzador.vbs` en el directorio de instalación. El agente lo lanza al arrancar (si existe). El script:
- Usa `guardian.lock` como mutex de instancia única (falla al abrir si ya hay otro VBS corriendo)
- Polling cada 2 segundos vía WMI (`Win32_Process`)
- Si `stop.lock` existe → `Exit Do` (apagado controlado)
- Si `print-agent.exe` no aparece → `shell.Run "print-agent.exe --revived"`

---

## 10. Logging

- **Librería:** `tracing` + `tracing-subscriber` + `tracing-appender`
- **Archivo:** `logs/agent.log` (rolling diario, un archivo por día)
- **Nivel configurable:** campo `log_level` en `config.toml` (`debug` | `info` | `warn`)
- **Sin consola:** `#![windows_subsystem = "windows"]` — el proceso es invisible

---

## 11. Compilación y Distribución

### Compilación

```powershell
$env:RUSTFLAGS = "-C target-feature=+crt-static"
cargo build --release --package agent-windows --target x86_64-pc-windows-msvc
```

El flag `crt-static` produce un binario sin dependencias de VC++ Redistributable. Solo enlaza APIs nativas de Windows (`kernel32.dll`, `ntdll.dll`, `user32.dll`).

### Packaging

```powershell
.\scripts\pack.ps1
```

**Genera en `dist/`:**

| Archivo | Descripción |
|---|---|
| `PrintAgentRS_Installer.exe` | Instalador Inno Setup completo |
| `version.txt` | `"{version} {sha256_lowercase}"` — metadata para OTA |
| `LEEME.txt` | Guía de campo para técnicos |

**Para publicar actualización OTA:** Subir `PrintAgentRS_Installer.exe` y `version.txt` al servidor. Enviar un mensaje a `update-air-{ambiente}` para distribuir inmediatamente.

### Instalador (`scripts/installer.iss`)

- **Sin privilegios de admin** (`PrivilegesRequired=lowest`)
- **Directorio:** `{localappdata}\PrintAgentRS`
- **Preserva `config.toml`** en actualizaciones (no sobreescribe la configuración existente)
- **Autoarranque:** `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
- **Genera `lanzador.vbs`** en `ssInstall` (antes del arranque post-instalación)
- **Desactiva guardian previo** escribiendo `stop.lock` antes de `taskkill` en upgrades
- **Página interactiva** solo en instalación limpia: solicita Ambiente, ID Cliente, ID Punto

---

## 12. Deuda Técnica Conocida

Ver `ARQUITECTURA.md` para análisis completo. Resumen de los ítems pendientes:

| # | Problema | Archivo | Impacto |
|---|---|---|---|
| 1 | `InsecureCertVerifier` — TLS deshabilitado efectivamente | `core/src/mqtt.rs:14` | Seguridad (MITM) |
| 2 | Comparación de versiones OTA lexicográfica | `core/src/updater.rs:28` | Bug latente en major version bump |
| 3 | Path temporal `temp/` hardcodeado en core | `core/src/mqtt.rs:274` | Bloqueador para Android |
| 4 | `updater.rs` en core pero es Windows-specific | `core/src/updater.rs` | Diseño incorrecto para multi-plataforma |
| 5 | Errores de impresión como strings libres | `core/src/mqtt.rs:233` | Observabilidad pobre |
| 6 | `broker_url()` con parsing manual complejo | `core/src/config.rs:87` | Riesgo de edge cases silenciosos |

---

## 13. Expansión a Android/iOS

La arquitectura ya está preparada. El trait `Plataforma` es el único punto de extensión necesario.

**Cambios requeridos antes de empezar Android:**
1. Agregar `fn directorio_temporal(&self) -> PathBuf` al trait (elimina path hardcodeado en core)
2. Agregar `async fn actualizar(&self, url: &str, version: &str) -> Result<bool>` al trait (OTA platform-specific)
3. Mover `updater.rs` a `agent-windows/` (no pertenece a core)

**Nuevo crate `agent-android/`:**
- `crate-type = ["cdylib"]` para UniFFI
- `AndroidPlatform` implementa `Plataforma` usando Android `PrintManager`
- Bridge Kotlin vía UniFFI (no JNI manual)
- Android Foreground Service reemplaza el tray icon

Ver [implementation_plan_mobile.md](implementation_plan_mobile.md) para el roadmap detallado.
