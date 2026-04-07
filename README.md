# PrintAgent RS 🚀

**PrintAgent RS** es un agente de impresión de alto rendimiento escrito en Rust, diseñado para operar como un servicio robusto en Windows. Su arquitectura modular permite una fácil portabilidad a otras plataformas como Android o iOS en el futuro.

## 📋 Características Principales

- **Arquitectura OS-Agnóstica:** Lógica de negocio separada de la implementación nativa mediante un sistema de Trait (`Plataforma`).
- **Comunicación MQTT:** Recibe comandos y reporta estados en tiempo real a través de un broker MQTT.
- **Motor de Impresión PDF:** Utiliza SumatraPDF para una impresión silenciosa, rápida y confiable.
- **Auto-Actualización Segura:** Sistema integrado de descarga con verificación de integridad criptográfica (SHA256).
- **Cero Dependencias (Static Runtime):** Compilación con enlace estático para funcionar en cualquier Windows "limpio" sin necesidad de Visual C++ Redistributable.
- **Resiliencia:** Manejo de errores estructurado, reconexión automática MQTT y limpieza de archivos temporales mediante RAII (Drop).

---

## 🏗️ Estructura del Proyecto (Workspace)

El proyecto está organizado como un Workspace de Rust para separar responsabilidades:

- **/core:** Contiene toda la lógica de negocio, protocolos de mensajes (JSON), el bucle de eventos MQTT y el motor de actualizaciones. **No contiene código específico de ningún SO.**
- **/agent-windows:** Implementación nativa para Windows. Contiene el cargador de configuración, el servicio del sistema y las llamadas FFI a la API de Win32 para gestionar impresoras.
- **/resources:** Directorio para binarios externos necesarios (ej. `SumatraPDF.exe`).
- **/scripts:** Herramientas de automatización para compilación y empaquetado.

---

## 🛠️ Requisitos de Desarrollo

Para compilar el proyecto en Windows, necesitas:

1. **Rust Toolchain:** Instalado vía [rustup](https://rustup.rs/). Asegúrate de usar el target `x86_64-pc-windows-msvc`.
2. **Visual Studio Build Tools:** Es necesario el enlazador `link.exe`.
   - Seleccionar la carga de trabajo: *"Desarrollo para el escritorio con C++"*.
   - Incluir: MSVC v143+ y Windows 10/11 SDK.
3. **SumatraPDF.exe:** Colocar una copia en la carpeta `resources/`.

---

## 🚀 Compilación y Empaquetado

Para generar una distribución lista para producción (Cero dependencias), utiliza el script de PowerShell incluido:

```powershell
.\scripts\pack.ps1
```

Este script:
1. Compila con `+crt-static` para eliminar dependencias de DLLs externas.
2. Genera la carpeta `dist/PrintAgentRS` con el ejecutable, recursos y configuración de ejemplo.
3. Crea un archivo `PrintAgentRS.zip` listo para despliegue.

---

## 📡 Protocolo de Comunicación (MQTT)

El agente escucha en el tópico: `{client_id}/comandos`

### Ejemplo: Listar Impresoras
**Request:**
```json
{ "action": "listPrinters" }
```
**Response:**
```json
{ "status": "ok", "printers": ["HP LaserJet Pro", "Microsoft Print to PDF"] }
```

### Ejemplo: Imprimir PDF
**Request:**
```json
{
  "action": "printPdf",
  "printerName": "HP LaserJet Pro",
  "pdfBase64": "JVBERi0xLjQK...",
  "jobId": "uuid-12345"
}
```

---

## 📱 Futuras Implementaciones (Android / iOS)

Gracias al diseño basado en el trait `Plataforma` en `core/src/traits.rs`, añadir soporte para una nueva plataforma es sencillo:

1. Crear un nuevo crate (ej. `agent-android`).
2. Implementar el trait `Plataforma` usando JNI para llamar al sistema de impresión de Android.
3. El código de `core` se reutiliza al 100% sin modificaciones, ya que el runtime MQTT y la lógica de archivos son estándar en Rust.

---

## 🛡️ Mejores Prácticas Aplicadas

- **Seguridad:** Validación de Hash SHA256 antes de aplicar cualquier actualización de binario.
- **Memoria:** Gestión de archivos temporales mediante `scopeguard` para asegurar que el disco se mantenga limpio incluso tras errores inesperados.
- **Logging:** Uso de `tracing` con rotación diaria de logs en la carpeta `./logs`.
- **Async:** Basado en `Tokio` para manejar múltiples comandos y la conexión de red de forma eficiente y no bloqueante.

---
*Desarrollado con enfoque en robustez y escalabilidad.*
