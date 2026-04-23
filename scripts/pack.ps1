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
if (Test-Path "resources\pdfium.dll") {
  Copy-Item "resources\pdfium.dll" $dist
}
else {
  Write-Host "!!! ADVERTENCIA: No se encontró resources\pdfium.dll. Agrégalo manualmente." -ForegroundColor Yellow
}

# 4. Generar LEEME.txt con instrucciones para soporte técnico de campo
$readme = "$dist\LEEME.txt"
@"
===============================================
 Agente AIR - GUIA RAPIDA PARA TECNICOS
===============================================

INSTALACION:
  Ejecutar PrintAgentRS_Installer.exe y seguir el asistente.
  El instalador pedira: Ambiente, ID Cliente e ID Punto.
  Al finalizar, el agente arranca automaticamente.

EL AGENTE CORRE EN SEGUNDO PLANO:
  No abre ventana. Buscar el icono en la Bandeja de Sistema
  (esquina inferior derecha, junto al reloj).
  Click derecho sobre el icono para ver las opciones.

ARRANQUE CON WINDOWS:
  El agente se registra automaticamente para iniciar
  cada vez que el usuario inicie sesion. No hay que
  configurar nada adicional.

CARPETA DE INSTALACION:
  %%localappdata%%\PrintAgentRS\

LOGS DE DIAGNOSTICO:
  %%localappdata%%\PrintAgentRS\logs\

PROBLEMAS FRECUENTES:
  - El agente no se conecta: Verificar que hay internet
    y que el broker MQTT esta en linea.
  - No imprime: Verificar que la impresora esta encendida
    y configurada como predeterminada en Windows.

  NO MODIFICAR config.toml manualmente.
  Si necesita cambiar credenciales, reinstale el agente.
"@ | Out-File -FilePath $readme -Encoding utf8

# 5. Compilar Asistente de Instalación con Inno Setup
$iscc = "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
if (Test-Path $iscc) {
  Write-Host "`n>>> Empaquetando Instalador Inno Setup (.exe)..." -ForegroundColor Cyan
  & $iscc .\scripts\installer.iss
  if ($LASTEXITCODE -eq 0) {
    Write-Host "`n>>> ÉXITO: Instalador generado en dist\PrintAgentRS_Installer.exe" -ForegroundColor Green
  }
  else {
    Write-Host "`n!!! Error al crear el instalador Inno Setup." -ForegroundColor Red
  }
}
else {
  Write-Host "!!! ADVERTENCIA: ISCC no encontrado en C:\Program Files (x86)\Inno Setup 6\ISCC.exe." -ForegroundColor Yellow
  Write-Host "Por favor instala Inno Setup 6 para empaquetar el ejecutable final." -ForegroundColor Yellow
}

# 6. Generar manifiesto de versión (version.txt) para Actualizaciones Automáticas (OTA)
$installerPath = "dist\PrintAgentRS_Installer.exe"
if (Test-Path $installerPath) {
  Write-Host "`n>>> Generando firma OTA (version.txt)..." -ForegroundColor Cyan
  # Extraemos la version directamente del Cargo.toml de agent-windows usando expresiones regulares
  $versionMatches = (Get-Content ".\agent-windows\Cargo.toml" -Raw) -match 'version\s*=\s*"([^"]+)"'
  $versionStr = if ($matches[1]) { $matches[1] } else { "1.0.0" }
    
  # Hasheo SHA256 estricto del archivo final
  $hashStr = (Get-FileHash -Path $installerPath -Algorithm SHA256).Hash.ToLower()
    
  # Escribir estructura requerida por updater.rs
  "$versionStr $hashStr" | Out-File -FilePath "dist\version.txt" -Encoding ascii
  Write-Host ">>> ÉXITO: dist\version.txt generado correctamente (v$versionStr)" -ForegroundColor Green
}
