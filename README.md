# Agente AIR 🚀

**Agente AIR** es un poderoso agente en segundo plano (Daemon) corporativo escrito en Rust. Está diseñado para operar invisiblemente desde la Bandeja de Sistema de Windows, recibir flujos de datos en la nube (MQTT) e imprimir documentos PDF sin interrupción.

La arquitectura fue rediseñada para ser robusta, auto-actualizable sin intervención técnica, y absolutamente inmune al temido ecosistema de requerimientos de permisos (Cero-UAC).

---

## 📋 Características de Clase Mundial

- **Daemon Invisible:** Corre directamente en el fondo (`#![windows_subsystem = "windows"]`) apoyado en un discreto ícono de la Bandeja de Sistema, eliminando consolas oscuras.
- **Actualizaciones Silenciosas (OTA):** Equipado con un mecanismo de *Zero-Downtime*. Descarga actualizaciones desde un Topic MQTT "Broadcast", verifica la integridad por Hash SHA-256 e invoca al Instalador de fondo. **No requiere Permisos de Administrador (Cero UAC)**.
- **Notificaciones Nativas Anti-Spam:** Se comunica con el usuario enviando alertas nativas Toast de Windows (WinRT) al ocurrir desconexiones graves y al reconectarse con el Broker.
- **Instalador Profesional (Inno Setup):** Un empaquetador `.exe` que guía visualmente al operario al pedir el *Ambiente* y las *Credenciales*, persistiendo esta configuración incluso ante desinstalaciones.
- **Cero Dependencias (Static Runtime):** Funciona al instante en cualquier computadora básica de Windows sin requerir instalación de perfiles como "Visual Studio C++".

---

## 🏗️ Flujo y Arquitectura del Proyecto

El código está compartimentado en un Workspace inmaculado para separar conceptos.

- **/core:** El núcleo intocable del sistema. Contiene el bucle asíncrono principal (Tokio + Rumqttc), procesadores de Mensajes, verificación criptográfica OTA (`updater.rs`) y todas las Interfaces Agnósticas al OS.
- **/agent-windows:** El cargador nativo. Inicia el Runtime de Tokio, captura el "Tray Icon" de Windows y procesa las llamadas crudas al sistema como `wmic` para impresoras, garantizando nulo parpadeo de ventanas (`CREATE_NO_WINDOW`).
- **/scripts:** Toda la magia de automatización CI/CD de un solo clic. Principalmente `pack.ps1` que engendra el instalador `.exe` final e imprime la firma `version.txt`.

---

## 🚀 Despliegue y Compilación (CI/CD Local)

Todo el despliegue a producción de esta arquitectura está resumido en un único comando asombroso:

1. Modifica la versión o el código fuente que desees alterar en `agent-windows/Cargo.toml`.
2. Abre la consola PowerShell en la raíz del proyecto.
3. Ejecuta el empaquetador mágico:
   ```powershell
   .\scripts\pack.ps1
   ```

**¿Qué hace este comando?**
Genera binarios estáticos absolutos, copia recursos PDF temporales, empaqueta el Instalador interactivo usando Inno Setup, deduce la versión compilada y te escupe dos elementos en tu carpeta `dist/`:
- `PrintAgentRS_Installer.exe`
- `version.txt`

Sube únicamente esos dos archivos resultantes a tu servidor web privado y ¡estás listo para lanzar el comando OTA de forma global!

---

## 📡 Protocolo Glocal (MQTT)

La arquitectura de conectividad es dual. Tienes **tópicos masivos** para administración global, y **tópicos locales** transaccionales.

### 1. Actualización Silenciosa Global (Broadcast)
El canal general escucha en modo fire-and-forget: `update-air-{ambiente}` (ej: `update-air-dev`).
Si envías un mensaje MQTT a ese topic, 1,000 computadores esparcidos en el país se actualizarán simultáneamente al servidor, reiniciándose en silencio en menos de 5 segundos.

### 2. Transaccional de Impresión y Listado
El agente local escucha peticiones en su ID: `{ambiente}-{id_cliente}-{id_punto}-imp-local`

**Comando (Listar Impresoras):**
```json
{ 
    "action": "listPrinters", 
    "responseTopic": "devolver-impresora-aqui" 
}
```

**Comando (Imprimir):**
```json
{
  "action": "printPdf",
  "printerName": "Caja 1 POS",
  "pdfBase64": "JVBERi0xLjQK...",
  "responseTopic": "devolver-status-aqui"
}
```

---

## 🛡️ Superpoderes del Ecosistema Config

Las credenciales están blindadas en el archivo `config.toml`. Si necesitas modificar una credencial de forma urgente o en caso de error:
1. Abre el block de notas hacia `C:\Users\TU_USUARIO\AppData\Local\PrintAgentRS\config.toml`.
2. Altera la variable a mano.
3. Haz clic derecho en el ícono del Agente (Esquina inferior derecha) y presiona **"Reiniciar Agente"**. El agente se "clonará" a sí mismo destruyendo a su yo del pasado, y reestablecerá su nueva vida en milisegundos con las configuraciones relucientes. ¡No más reiniciar el PC entero!

---
*Desarrollado y mantenido con estándares de máxima resiliencia arquitectónica.*
