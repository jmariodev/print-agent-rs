# Plan de Expansión a Android/iOS

Hoja de ruta para extender Agente AIR al ecosistema móvil, reutilizando el núcleo Rust existente.

---

## Fundamento Arquitectónico

El proyecto ya está diseñado para esto. El trait `Plataforma` en `core/src/traits.rs` es el contrato que separa la lógica de negocio (MQTT, cifrado, OTA) de la implementación de plataforma (Win32, Android PrintManager, etc.).

**Lo que se reutiliza sin cambios (~70% del trabajo):**
- `core/src/mqtt.rs` — Event loop MQTT completo
- `core/src/config.rs` — Tipos `Config` y `Ambiente`
- `core/src/crypto.rs` — AES-256-GCM
- `core/src/messages.rs` — Comandos serde

**Lo que hay que escribir para Android:**
- `agent-android/src/platform.rs` — `AndroidPlatform` impl `Plataforma`
- `agent-android/src/printer_android.rs` — Android `PrintManager` o BLE/WiFi Direct
- Bridge UniFFI para Kotlin
- Android Foreground Service (reemplaza el tray icon)
- `WorkManager` o `JobScheduler` (reemplaza el guardian VBScript)
- OTA via `Intent` de instalación APK (reemplaza Inno Setup)

---

## Cambios Previos Requeridos en `core/`

Estos cambios son necesarios antes de empezar el crate Android. Son pequeños y no rompen nada en Windows:

### 1. Agregar `directorio_temporal()` al trait

El path `temp/` está hardcodeado en `core/src/mqtt.rs:274`. En Android no existe `./temp/`.

```rust
// core/src/traits.rs — agregar al trait Plataforma
fn directorio_temporal(&self) -> std::path::PathBuf;
```

```rust
// agent-windows/src/platform.rs
fn directorio_temporal(&self) -> PathBuf { PathBuf::from("temp") }

// agent-android/src/platform.rs (futuro)
fn directorio_temporal(&self) -> PathBuf {
    PathBuf::from("/data/data/com.gama.airagent/cache/print")
}
```

### 2. Agregar `actualizar()` al trait y mover `updater.rs` fuera de core

`core/src/updater.rs` descarga y ejecuta un instalador Inno Setup `.exe` — es código Windows-only que no pertenece en `core/`.

```rust
// core/src/traits.rs — agregar al trait Plataforma
async fn actualizar(&self, update_url: &str, version_actual: &str) -> Result<bool>;
```

- `agent-windows` implementa: descarga `.exe`, verifica SHA-256, ejecuta Inno Setup
- `agent-android` implementa: descarga `.apk`, verifica SHA-256, lanza `Intent` de instalación
- La lógica compartida (fetch `version.txt`, comparación de versión, streaming + hash) va a `core/src/update_utils.rs`

### 3. Corregir comparación de versiones

La comparación actual es lexicográfica y falla si el major version sube (`"9.0.0" > "26.1.1"`). Resolver antes de que esto afecte al móvil también.

---

## Estructura del Workspace Final

```
print-agent-rs/
├── core/                         # Sin cambios grandes (+ directorio_temporal, + actualizar en trait)
│   └── src/
│       ├── update_utils.rs       # NUEVO: lógica OTA portable (sin Inno Setup)
│       └── traits.rs             # + directorio_temporal() + actualizar()
│
├── agent-windows/                # Sin cambios en funcionalidad
│   └── src/
│       └── updater_windows.rs    # MOVIDO desde core/src/updater.rs
│
└── agent-android/                # NUEVO crate
    ├── Cargo.toml
    │   # [lib]
    │   # crate-type = ["cdylib"]
    │   # [dependencies]
    │   # agent-core = { path = "../core" }
    │   # uniffi = "0.27"
    │   # jni = "0.21"
    └── src/
        ├── lib.rs                # uniffi::include_scaffolding! + exports públicos
        ├── platform.rs           # AndroidPlatform impl Plataforma
        └── printer_android.rs    # Android PrintManager API
```

---

## Stack Tecnológico

### Rust Core → Kotlin via UniFFI

UniFFI (Mozilla) genera el bridge automáticamente a partir de definiciones Rust. No se escribe JNI manual.

```rust
// agent-android/src/lib.rs
uniffi::include_scaffolding!("agent_android");

#[uniffi::export]
pub fn iniciar_agente(config_toml: String) -> Result<(), AgentError> {
    let runtime = tokio::runtime::Runtime::new()?;
    let cfg = /* parsear config_toml */;
    let plataforma = Arc::new(AndroidPlatform::new());
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    runtime.block_on(mqtt::run(cfg, plataforma, shutdown_rx, /* pause_rx */))?;
    Ok(())
}
```

```kotlin
// Android Foreground Service
class PrintAgentService : Service() {
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        thread { AgentAndroid.iniciarAgente(leerConfigToml()) }
        return START_STICKY
    }
}
```

### Renderizado PDF en Android — Decisión Pendiente

| Opción | Pros | Contras |
|---|---|---|
| **A. `pdfium` compilado para ARM64** | Mismo pipeline que Windows, misma calidad | Compilar `libpdfium.so` para Android tiene friction; +8 MB al APK |
| **B. Servidor renderiza, Android recibe imagen** | APK liviano, sin dependencias nativas | Cambia el protocolo MQTT; el servidor necesita capacidad de renderizado |

**Criterio de decisión:** Si el volumen de impresión es alto y la calidad es crítica → Opción A. Si el APK debe ser mínimo y el servidor tiene capacidad → Opción B.

---

## Hoja de Ruta de Implementación

| Paso | Tarea | Notas |
|---|---|---|
| 1 | Agregar `directorio_temporal()` al trait | Pequeño, bajo riesgo, desbloquea todo |
| 2 | Corregir comparación semver en OTA | Una función auxiliar de 5 líneas |
| 3 | Mover `updater.rs` a `agent-windows/` + `update_utils.rs` en core | Refactor de ~1h |
| 4 | Crear `agent-android/` con `AndroidPlatform` stub que compile | Validar que el workspace compila con el nuevo crate |
| 5 | Implementar `printer_android.rs` con Android `PrintManager` | Decidir estrategia PDF antes de este paso |
| 6 | Integrar UniFFI + Android Foreground Service en Kotlin | Validar bridge con un log simple |
| 7 | Implementar `actualizar()` para Android (APK Intent) | Requiere permisos `REQUEST_INSTALL_PACKAGES` |
| 8 | CI/CD: GitHub Actions compila `.so` ARM64 + empaqueta APK | `cargo build --target aarch64-linux-android` |

---

## CI/CD Pipeline (GitHub Actions)

```yaml
# .github/workflows/android.yml
jobs:
  build-android:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: aarch64-linux-android
      - name: Install Android NDK
        uses: android-actions/setup-android@v3
      - name: Build Rust .so
        run: cargo build --release --package agent-android --target aarch64-linux-android
      - name: Copy .so to KMP jniLibs
        run: cp target/.../libagent_android.so android-app/src/main/jniLibs/arm64-v8a/
      - name: Build APK
        run: ./gradlew assembleRelease
```

**Resultado:** Push de código Rust → robot compila → APK generado automáticamente → distribuible vía Play Store o link directo.

---

## Consideraciones de Plataforma

- **OTA en Android:** Google Play Store y Apple App Store prohíben actualización binaria silenciosa. En Android se puede usar `Intent` con `ACTION_VIEW` para instalar APKs (requiere permiso del usuario), o distribuir por Play Store y usar in-app update API.
- **Guardian en Android:** `WorkManager` con `PeriodicWorkRequest` o un Foreground Service con `START_STICKY` mantiene el agente vivo sin necesidad de polling manual.
- **Notificaciones:** Reemplazar Toast WinRT con `NotificationCompat` de Android y un canal de notificación persistente.
- **Cifrado de config:** `AES-256-GCM` de `core/src/crypto.rs` funciona igual en Android. Sin cambios.
