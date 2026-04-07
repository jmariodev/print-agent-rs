# scripts/pack.ps1
# ─────────────────────────────────────────────────────────────────────────────
# Genera el binario estático y empaqueta la distribución final
# ─────────────────────────────────────────────────────────────────────────────

# 1. Limpiar o crear carpeta de distribución
$dist = "dist\PrintAgentRS"
if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
New-Item -ItemType Directory -Force $dist | Out-Null

Write-Host ">>> Compilando Agent-Windows (Enlace estático)..." -ForegroundColor Cyan
# Forzar crt-static para eliminar dependencia de vcruntime140.dll
$env:RUSTFLAGS = "-C target-feature=+crt-static"
cargo build --release --package agent-windows --target x86_64-pc-windows-msvc

if ($LASTEXITCODE -ne 0) {
    Write-Host "!!! Error en la compilación. Revisa los logs." -ForegroundColor Red
    exit 1
}

# 2. Copiar binario generado
$binPath = "target\x86_64-pc-windows-msvc\release\agent-windows.exe"
Copy-Item $binPath "$dist\print-agent.exe"

# 3. Copiar recursos
Write-Host ">>> Copiando recursos..." -ForegroundColor Cyan
if (Test-Path "resources\SumatraPDF.exe") {
    Copy-Item "resources\SumatraPDF.exe" $dist
} else {
    Write-Host "!!! ADVERTENCIA: No se encontró resources\SumatraPDF.exe. Agrégalo manualmente." -ForegroundColor Yellow
}

# 4. Copiar plantilla de configuración
$configExample = "$dist\config.toml.example"
@"
# PrintAgent RS — Configuración a prueba de tontos
ambiente   = "dev"       # dev | test | prod
id_cliente = "clienteX"
id_punto   = "puntoY"

# VARIABLES AVANZADAS (Puedes dejarlas, o borrarlas y el sistema usará las fijas)
broker_url = "mqtt://127.0.0.1"
broker_port = 1883
update_url = "https://updates.tudominio.com/print-agent/"
log_level  = "info"
"@ | Out-File -FilePath $configExample -Encoding utf8

# 5. Generar config.toml listos para editar
$configBlank = "$dist\config.toml"
@"
# COMPLETA ESTOS DATOS OBLIGATORIOS PARA CONECTAR EL AGENTE
ambiente   = "prod"   # dev | test | prod
id_cliente = ""
id_punto   = ""
"@ | Out-File -FilePath $configBlank -Encoding utf8

# 6. Generar LEEME.txt con instrucciones de configuracion
$readme = "$dist\LEEME.txt"
@"
=== INSTRUCCIONES DE CONFIGURACION PRINTAGENT RS ===
Para instalar y poner a correr este agente:
1. Abre 'config.toml' y rellena el ambiente, id_cliente y id_punto.
2. Si requieres parametros de red avanzados como forzar una nueva URL del broker MQTT para pruebas locales, puedes guiarte leyendo el archivo 'config.toml.example'.
3. Instala e inicia el Agente en formato de Servicio de Windows usando la ruta de este ejecutable.
"@ | Out-File -FilePath $readme -Encoding utf8

# 7. Comprimir distribución
$zipFile = "dist\PrintAgentRS.zip"
if (Test-Path $zipFile) { Remove-Item $zipFile }
Compress-Archive -Path "$dist\*" -DestinationPath $zipFile -Force

Write-Host "`n>>> ÉXITO: Distribución generada en $zipFile" -ForegroundColor Green
Write-Host "Contenido del ZIP:"
Get-ChildItem $dist | Select-Object Name
