# Contexto del Proyecto: Agente AIR

## Visión General
Agente de impresión en Rust que opera como servicio. Recibe comandos por MQTT y ejecuta impresiones PDF usando SumatraPDF (en Windows). Diseñado para ser extensible a plataformas móviles (Android/iOS).

## Estado Actual
- [x] Especificaciones técnicas definidas en `PrintAgent_RS_Specs.md`.
- [x] Estructura de Workspace de Rust configurada.
- [x] Implementación de `core` (renombrado a `agent-core` para evitar colisiones con la librería estándar).
- [x] Implementación de `agent-windows`.

## Reglas de Oro (Basado en Specs)
1. **Portabilidad:** La lógica en `core/` NUNCA debe depender de APIs de SO específicas. Todo lo nativo va tras el trait `Plataforma`.
2. **Robustez:** "Fail fast, recover gracefully". Todo error se loguea y se reporta por MQTT si es posible.
3. **Seguridad:** Los ejecutables descargados deben verificarse con SHA256.
4. **Limpieza:** Uso de RAII (Drop) para asegurar que los PDFs temporales se borren siempre.

## Próximos Pasos
1. Realizar pruebas de integración con el broker MQTT.
2. Comprobar la correcta descarga e instanciamiento de la actualización remota (`updater.rs`).
3. Comprobar impresión de base64 con `SumatraPDF`.
