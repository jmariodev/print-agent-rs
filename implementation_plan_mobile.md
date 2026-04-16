# 📱 Plan de Expansión Estratégica: Ecosistema Móvil Cross-Platform

Este documento detalla la arquitectura oficial a seguir para el futuro desarrollo de **Agente AIR Móvil** (Android/iOS), garantizando la reutilización algorítmica y maximizando la eficiencia de batería en los teléfonos y pasarelas de pago POS.

> [!IMPORTANT]
> **Pilar Arquitectónico**: La aplicación actual heredará al 100% sus entrañas. No reescribiremos lógica concurrente ni enrutamiento. La meta es inyectar un cascarón cross-platform (Kotlin) que invoque estáticamente al núcleo nativo (Rust).

---

## 1. El Santo Grial: El Stack Tecnológico (Rust + UniFFI + KMP)

### Componente A: El Núcleo Back-End (`agent-core`)
Seguirá siendo la única fuente de verdad sobre cómo los datos viajan:
- Contendrá la persistencia, conexión a la Nube (Broker MQTT) a través de `rumqttc`, y los Handlers de serialización JSON.
- **Acción:** Apagar quirúrgicamente el actualizador autómata OTA (`#[cfg(target_os="windows")]` sobre `updater.rs`), ya que la Apple AppStore y Google Play Store imponen sus propias reglas de actualización binaria anti-malware.

### Componente B: El "Cable" de Traducción (`UniFFI`)
El ecosistema de Mozilla incluye **`uniffi`**, un generador de bindings automáticos.
- **Acción:** Crearemos el Crate `agent-mobile`. Este Crate usará `uniffi` para leer el código de Rust y vomitar un archivo 100% compatible con Kotlin (Android) y Swift (iOS LLVM).

### Componente C: El Front-End Multiplataforma (KMP)
- Se escribirá la interfaz visual unificada con **Kotlin Multiplatform (Compose)**.
- Se implementará un **Foreground Service** móvil clavado en la barra de Android (para que el sistema operativo no mate la aplicación por ahorrar batería), el cual instanciará las lógicas que arroje Rust.

---

## 2. Flujo de Trabajo en el Día a Día (CI/CD Pipeline)

Al desactivar las instalaciones silenciosas clandestinas de Windows (OTA), el ciclo de actualización de un dispositivo móvil se basará estrictamente en empaquetar Nuevos `APKs` (o `AABs`) para desplegar en las tiendas.

Para no hacer el proceso traumático y manual, montaremos una "Línea de Ensamblaje Robótica" **(CI/CD Pipeline)** usando herramientas gratuitas como *GitHub Actions*.

### Así será el proceso automatizado cuando desees hacer un cambio:
1. **Modificas Rust (El Back):** Editas una regla matemática en `agent-core` desde tu computadora y haces un "Push" a tu nube de código (GitHub).
2. **El Servidor Ensambla los Binarios:**  
   Un robot en la nube automáticamente corre el comando de compilación remota (`cargo build --target aarch64-linux-android`) que escupe los archivos ocultos `.so` (librerías puras).
3. **El Servidor Compila KMP (El Front):** 
   Ese mismo robot inyecta automáticamente los `.so` en las carpetas `/jniLibs` de tu proyecto de Kotlin (Android Studio Virtual) y genera las traducciones puente.
4. **Emisión y Distribución:**  
   Inmediatamente el robot compila el proyecto completo de KMP, escupiendo un `App_v2.apk` listo para usarse. Ese link se enviará automáticamente a tu Dashboard (o se sube solo a la consola de Google Play Store).

**Resultado:** Tú escribes código en Rust y subes el commit; el robot hace el resto por ti y tus clientes recibirán el `.apk` actualizado a los minutos.

---

## 3. Hoja de Ruta Estructural

### Fase I: Refactorización y Enlazado C / FFI
1. Agregar un workspace secundario: `agent-mobile` de tipo `cdylib` (C Dynamic Library).
2. Definir una interfaz estándar con UniFFI donde documentaremos los comandos que Kotlin le puede gritar a Rust: `encender_agente(id_punto, id_cliente)`, `apagar_agente()`.
3. Validar exportación inicial hacia un proyecto base en Android Studio probando logs.

### Fase II: Implementación nativa de la Interfaz "Plataforma"
Dentro del núcleo KMP aplicaremos el patrón arquitectónico "Expect / Actual":
- **En la capa compartida (KMP):** Interceptamos el `Base64` del PDF que escupió Rust.
- **En Android (`actual`):** Se invoca la API `PrintManager` oficial, o se abre un túnel por *Bluetooth Serial / BLE* dependiendo del hardware portátil de facturación.
- **En iOS (`actual`):** Se canaliza ese Array al ecosistema `UIPrintInteractionController`.

---

> [!TIP]
> Guarda la dirección IP de `uniffi` en Mozilla para tus desarrolladores: https://mozilla.github.io/uniffi-rs/. Junto al ecosistema de **KMP**, conforma la tecnología militar estándar que se usará el día que decidamos cruzar al hardware telefónico.
