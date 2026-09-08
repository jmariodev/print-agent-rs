# API MQTT — Agente AIR

Protocolo de comunicación entre el servidor/aplicación y los agentes de impresión.

---

## Topics

El agente se suscribe automáticamente a tres topics al conectarse:

| Topic | Formato | Uso |
|---|---|---|
| **Transaccional** | `{ambiente}-{id_cliente}-{id_punto}-imp-local` | Comandos dirigidos a un agente específico |
| **Broadcast OTA** | `update-air-{base_env}` | Actualización masiva a todos los agentes del ambiente |
| **Status por cliente** | `{ambiente}-{id_cliente}-air-status` | Consulta de estado de todos los agentes de un cliente |
| **Eventos por cliente** | `{ambiente}-{id_cliente}-air-event` | Eventos en tiempo real (online/offline) de los agentes del cliente |

### Ambientes y `base_env`

El campo `ambiente` en `config.toml` puede ser: `dev`, `test`, `prod`, o cualquier variante `prod_*` (ej: `prod_gd4`, `prod_bog`). Todos los ambientes `prod_*` comparten el mismo canal broadcast `update-air-prod`.

| `ambiente` configurado | Topic transaccional | Topic broadcast OTA |
|---|---|---|
| `dev` | `dev-{cliente}-{punto}-imp-local` | `update-air-dev` |
| `test` | `test-{cliente}-{punto}-imp-local` | `update-air-test` |
| `prod` | `prod-{cliente}-{punto}-imp-local` | `update-air-prod` |
| `prod_gd4` | `prod_gd4-{cliente}-{punto}-imp-local` | `update-air-prod` |
| `prod_bog` | `prod_bog-{cliente}-{punto}-imp-local` | `update-air-prod` |

**Ejemplo** con `ambiente=prod_gd4`, `id_cliente=gama`, `id_punto=001`:
- Transaccional: `prod_gd4-gama-001-imp-local`
- Broadcast: `update-air-prod`
- Status por cliente: `prod_gd4-gama-air-status`
- Eventos por cliente: `prod_gd4-gama-air-event`

---

## Comandos Transaccionales

Se envían al topic `{ambiente}-{id_cliente}-{id_punto}-imp-local` en formato JSON.

Todos los comandos requieren el campo `responseTopic` donde el agente publicará la respuesta.

---

### 1. Listar Impresoras

Retorna las impresoras instaladas en el equipo del agente (locales y de red).

**Enviar:**
```json
{
  "action": "listPrinters",
  "responseTopic": "mi-app/respuestas/impresoras"
}
```

**Respuesta** (en `responseTopic`):
```
[EPSON TM-T20III, HP LaserJet Pro, Microsoft Print to PDF]
```

---

### 2. Imprimir PDF

Envía un PDF codificado en Base64 a una impresora específica. El agente lo renderiza con pdfium + GDI y envía un corte ESC/POS al terminar (impresoras térmicas).

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
| `responseTopic` | string | Topic donde el agente enviará el resultado |
| `printerName` | string | Nombre exacto de la impresora (usar `listPrinters` para obtenerlo) |
| `fileToPrint` | string | PDF codificado en Base64 (máximo ~15 MB efectivos; límite MQTT: 20 MB) |

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

Solicita a un agente específico que verifique si hay nueva versión disponible.

**Enviar:**
```json
{
  "action": "update-air",
  "responseTopic": "mi-app/respuestas/update",
  "ambiente": "prod"
}
```

| Campo | Tipo | Descripción |
|---|---|---|
| `action` | string | Siempre `"update-air"` |
| `responseTopic` | string | Topic donde el agente confirmará el inicio |
| `ambiente` | string (opcional) | Ambiente del servidor de actualización. Si se omite, usa el configurado en el agente |

**Respuesta inmediata** (la instalación ocurre en background):
```
Verificando actualizaciones
```

> Si hay nueva versión, el agente la descarga, verifica SHA-256 e instala silenciosamente. El proceso finaliza con `exit(0)` para liberar file locks antes de que Inno Setup sobrescriba el binario.

---

### 4. Consulta de Estado por Cliente (`air-status`)

Solicita a **todos los agentes de un cliente** que reporten su identidad. Solo responden los agentes que están encendidos; los que no responden se asumen apagados.

**Topic:** `{ambiente}-{id_cliente}-air-status`

**Enviar:**
```json
{
  "action": "air-status",
  "responseTopic": "mi-app/respuestas/air-status"
}
```

| Campo | Tipo | Descripción |
|---|---|---|
| `action` | string | Siempre `"air-status"` |
| `responseTopic` | string | Topic donde cada agente encendido publicará su identidad |

**Respuesta** (en `responseTopic`, una por cada agente encendido):
```json
{
  "idCliente": "gama",
  "idPunto": "001",
  "ambiente": "prod_gd4",
  "estado": "encendido"
}
```

> El `responseTopic` debe ser único por consulta para evitar cruce de respuestas entre solicitudes concurrentes (por ejemplo, incluyendo un UUID).

---

### 5. Eventos en Tiempo Real por Cliente (`air-event`)

El agente publica de forma **proactiva** su estado en el topic `{ambiente}-{id_cliente}-air-event`, permitiendo que un panel suscrito a ese topic se actualice en tiempo real sin necesidad de consultas.

**Eventos publicados:**

| Evento | Cuándo se publica |
|---|---|
| `online` | Cuando el agente establece (o restablece) conexión con el broker, o se reanuda de una pausa |
| `offline` | Publicado automáticamente por el broker (Last Will & Testament) cuando el agente se cae o pierde conexión inesperadamente |

**Payload** (en el topic `{ambiente}-{id_cliente}-air-event`):
```json
{
  "evento": "online",
  "idCliente": "gama",
  "idPunto": "001",
  "ambiente": "prod_gd4",
  "estado": "online"
}
```

> Para `offline`, el broker usa el **Last Will & Testament** registrado por el agente al conectarse, por lo que el evento se publica incluso si el proceso muere o se corta la red de forma abrupta (siempre que el broker detecte la desconexión).

---

## Broadcast OTA (Actualización Masiva)

Actualiza **todos los agentes** de un ambiente simultáneamente con un solo mensaje.

**Topic:** `update-air-dev` | `update-air-test` | `update-air-prod`

**Payload:** Cualquier contenido o vacío. El agente ignora el body y ejecuta la verificación automáticamente.

> Los agentes en ambientes `prod_*` (ej: `prod_gd4`, `prod_bog`) escuchan todos el mismo topic `update-air-prod`.

**Flujo:**
1. Se publica cualquier mensaje en `update-air-prod`
2. Todos los agentes con ambiente `prod*` reciben la señal
3. Cada agente descarga `version.txt` del servidor de actualizaciones
4. Si hay versión nueva: descarga `PrintAgentRS_Installer.exe`, verifica SHA-256, instala silenciosamente
5. El agente se reinicia con la nueva versión (el Guardian VBS lo revive automáticamente)

> El broadcast se procesa incluso si el agente está en modo **Pausa** (solo los trabajos de impresión se ignoran durante la pausa).

---

## Manejo de Errores

Si el JSON enviado es inválido o falta el campo `action`, el agente responde al `responseTopic` (si puede extraerlo del JSON malformado):

```
Action is required
```

Si el JSON está tan corrupto que no se puede extraer el `responseTopic`, el error se registra solo en los logs del agente (`logs/agent.log`).

---

## Notas de Implementación

- **QoS:** El agente suscribe y publica con `QoS::AtLeastOnce` (QoS 1).
- **Persistencia de suscripción:** Al reconectarse al broker, el agente re-suscribe automáticamente a ambos topics.
- **Concurrencia:** Cada mensaje entrante se procesa en su propio `tokio::spawn`. El mutex global de impresión `PRINT_MUTEX` garantiza que dos trabajos de impresión no se solapen si llegan en ráfaga.
- **Tamaño máximo de payload:** 20 MB (configurable en `mqtt.rs`).
