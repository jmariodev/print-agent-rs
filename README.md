# Agente AIR

Agente de impresión corporativo escrito en Rust. Corre como daemon invisible en Windows, recibe trabajos de impresión desde la nube vía MQTT y los ejecuta en impresoras físicas usando la API nativa de Windows (Win32 GDI). Diseñado para despliegue masivo en puntos de venta (POS) sin intervención del usuario.

---

## Características

- **Daemon invisible:** Corre en segundo plano con ícono en la bandeja del sistema. Sin ventanas de consola.
- **Impresión nativa Win32:** Renderiza PDFs vía `pdfium` + GDI directamente al spooler de Windows. Sin dependencias externas de terceros (no SumatraPDF, no PowerShell).
- **Corte automático de papel:** Envía comando ESC/POS de corte al terminar cada impresión (impresoras térmicas).
- **Actualizaciones OTA silenciosas:** Descarga el instalador, verifica integridad SHA-256 y ejecuta Inno Setup sin intervención del usuario. Sin permisos de administrador (Cero UAC).
- **Alta disponibilidad con Guardian:** Un script VBScript generado durante la instalación revive el agente automáticamente si cae inesperadamente.
- **Configuración encriptada:** En ambientes de producción (`prod*`), el `config.toml` se cifra automáticamente con AES-256-GCM al primer arranque.
- **Notificaciones Toast nativas:** Informa al usuario sobre conexiones, desconexiones y reconexiones vía notificaciones nativas de Windows.
- **Zero dependencias de runtime:** Binario estático (sin Visual C++ Redistributable requerido).

---

## Arquitectura del Workspace

```
print-agent-rs/
├── core/           # Lógica cross-platform (sin código Win32, sin unsafe)
│   └── src/
│       ├── config.rs       # Tipos Config y Ambiente con validación
│       ├── crypto.rs       # Cifrado AES-256-GCM para config.toml
│       ├── messages.rs     # Structs serde para comandos MQTT
│       ├── mqtt.rs         # Event loop MQTT y dispatcher de comandos
│       ├── traits.rs       # Trait Plataforma (contrato OS-agnóstico)
│       └── updater.rs      # OTA: descarga, SHA-256, Inno Setup silencioso
│
└── agent-windows/  # Implementación Windows-específica
    └── src/
        ├── main.rs             # Entry point, tray icon, logging, canales tokio
        ├── config_loader.rs    # Carga config.toml con auto-cifrado en prod
        ├── platform.rs         # WindowsPlatform — impl del trait Plataforma
        └── printer_win.rs      # Win32: EnumPrintersW + pdfium GDI + ESC/POS
```

**Regla de dependencias:** `core` no depende de `agent-windows`. `agent-windows` depende de `core`. Esta separación es intencional para permitir futura expansión a Android/iOS.

---

## Configuración (`config.toml`)

El instalador genera el archivo automáticamente. En producción se cifra con AES-256-GCM al primer arranque.

```toml
ambiente    = "prod_gd4"   # dev | test | prod | prod_* (ej: prod_gd4, prod_bog)
id_cliente  = "gama"
id_punto    = "001"

# Opcionales — tienen valores por defecto si se omiten
broker_url  = "wss://broker.ejemplo.com/mqtt"
broker_port = 8883
update_url  = "https://updates.ejemplo.com/ActualizadorAIR/prod/"
log_level   = "info"       # debug | info | warn
```

El agente construye automáticamente los tópicos MQTT a partir de estos valores.

Para editar la configuración en equipos de desarrollo/prueba: clic derecho en el ícono de la bandeja → **Cambiar Configuración** (desencripta, abre Notepad, re-encripta al reiniciar).

---

## Compilación y Empaquetado

```powershell
# Desde la raíz del proyecto
.\scripts\pack.ps1
```

El script genera en `dist/`:
- `PrintAgentRS_Installer.exe` — Instalador completo (Inno Setup)
- `version.txt` — `"{version} {sha256}"` usado por el sistema OTA

Para publicar una actualización: sube ambos archivos al servidor de actualizaciones. Los agentes conectados la recibirán automáticamente vía broadcast MQTT o al reiniciarse.

---

## Protocolo MQTT

Ver [API_MQTT.md](API_MQTT.md) para la referencia completa del protocolo.

### Topics

| Topic | Uso |
|---|---|
| `{ambiente}-{id_cliente}-{id_punto}-imp-local` | Comandos directos al agente |
| `update-air-{base_env}` | Actualización OTA masiva (broadcast) |

**Ejemplo** con `ambiente=prod_gd4`, `id_cliente=gama`, `id_punto=001`:
- Comandos: `prod_gd4-gama-001-imp-local`
- Broadcast OTA: `update-air-prod` (todos los `prod*` comparten este canal)

### Comandos disponibles

```json
{ "action": "listPrinters", "responseTopic": "..." }
{ "action": "print", "responseTopic": "...", "printerName": "...", "fileToPrint": "<base64_pdf>" }
{ "action": "update-air", "responseTopic": "...", "ambiente": "prod" }
```

---

## Sistema de Alta Disponibilidad

El instalador genera un script `lanzador.vbs` que:
- Corre en background con polling cada 2 segundos
- Si `stop.lock` existe → se detiene (apagado controlado)
- Si `print-agent.exe` no está corriendo → lo relanza con `--revived`

El agente registra en logs cuando es resucitado por el guardian.

Para apagado controlado: usar **Cerrar Agente** desde el menú de bandeja (crea `stop.lock` antes de salir). Para desinstalación: usar **Desinstalar Agente** (también crea `stop.lock` antes de ejecutar el uninstaller).

---

## Documentación Técnica

| Archivo | Contenido |
|---|---|
| [API_MQTT.md](API_MQTT.md) | Protocolo MQTT completo con ejemplos |
| [PrintAgent_RS_Specs.md](PrintAgent_RS_Specs.md) | Especificaciones técnicas para desarrolladores |
| [implementation_plan_mobile.md](implementation_plan_mobile.md) | Roadmap de expansión a Android/iOS |
