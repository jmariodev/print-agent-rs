# 📡 API MQTT — PrintAgent RS

Documentación del protocolo de comunicación entre el servidor y los agentes de impresión.

---

## Tópicos

El agente se suscribe automáticamente a **dos tópicos** al conectarse:

| Tópico | Formato | Uso |
|---|---|---|
| **Transaccional** | `{ambiente}-{idCliente}-{idPunto}-imp-local` | Comandos dirigidos a un agente específico |
| **Broadcast** | `update-air-{ambiente}` | Actualización masiva OTA a todos los agentes del ambiente |

**Ejemplo:** Un agente con `ambiente=dev`, `id_cliente=118`, `id_punto=285` escucha:
- `dev-118-285-imp-local` (comandos directos)
- `update-air-dev` (actualizaciones globales)

---

## Comandos Transaccionales

Se envían al tópico `{ambiente}-{idCliente}-{idPunto}-imp-local` en formato JSON.

Todos los comandos requieren el campo `responseTopic` para que el agente responda al emisor.

---

### 1. Listar Impresoras

Obtiene las impresoras instaladas en el equipo del agente.

**Enviar:**
```json
{
  "action": "listPrinters",
  "responseTopic": "mi-app/respuestas/impresoras"
}
```

**Respuesta** (en `mi-app/respuestas/impresoras`):
```
[HP LaserJet Pro, EPSON TM-T20III, Microsoft Print to PDF]
```

---

### 2. Imprimir PDF

Envía un archivo PDF codificado en Base64 a una impresora específica.

**Enviar:**
```json
{
  "action": "print",
  "responseTopic": "mi-app/respuestas/impresion",
  "printerName": "EPSON TM-T20III",
  "fileToPrint": "JVBERi0xLjQKMSAwIG9iago8PA0vUGFn..."
}
```

| Campo | Tipo | Descripción |
|---|---|---|
| `action` | string | Siempre `"print"` |
| `responseTopic` | string | Tópico donde el agente enviará el resultado |
| `printerName` | string | Nombre exacto de la impresora (tal como aparece en `listPrinters`) |
| `fileToPrint` | string | Contenido del PDF codificado en Base64 |

**Respuesta exitosa:**
```
Impresion exitosa
```

**Respuesta con error:**
```
Error al imprimir
```

---

### 3. Actualización Dirigida

Solicita al agente que verifique si hay una nueva versión disponible.

**Enviar:**
```json
{
  "action": "update-air",
  "responseTopic": "mi-app/respuestas/update",
  "ambiente": "dev"
}
```

| Campo | Tipo | Descripción |
|---|---|---|
| `action` | string | Siempre `"update-air"` |
| `responseTopic` | string | Tópico donde el agente confirmará |
| `ambiente` | string (opcional) | Ambiente del servidor de actualización. Si se omite, usa el configurado |

**Respuesta:**
```
Verificando actualizaciones
```

> El agente descarga e instala silenciosamente en segundo plano si hay versión nueva.

---

## Comando Broadcast (Actualización Masiva OTA)

Se envía al tópico `update-air-{ambiente}` para actualizar **todos los agentes** de un ambiente simultáneamente.

**Tópico:** `update-air-dev` | `update-air-test` | `update-air-prod`

**Payload:** Cualquier contenido (puede estar vacío). El agente ignora el body y ejecuta la verificación de actualización automáticamente.

**Flujo:**
1. Se publica cualquier mensaje en `update-air-dev`
2. Todos los agentes del ambiente `dev` reciben la señal
3. Cada agente descarga `version.txt` del servidor de actualizaciones
4. Si hay versión nueva: descarga el instalador, verifica SHA-256, instala silenciosamente
5. El agente se reinicia con la nueva versión

---

## Errores

Si el JSON enviado es inválido o le falta el campo `action`, el agente responde al `responseTopic` (si existe) con:

```
Action is required
```

Si no hay `responseTopic`, el error se registra solo en los logs del agente.
