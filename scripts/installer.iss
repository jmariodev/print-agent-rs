[Setup]
AppName=Agente AIR
AppVersion=1.0.0
DefaultDirName={localappdata}\PrintAgentRS
DefaultGroupName=Agente AIR
OutputDir=..\dist
OutputBaseFilename=PrintAgentRS_Installer
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
DisableProgramGroupPage=yes

[Files]
Source: "..\dist\PrintAgentRS\*"; DestDir: "{app}"; Excludes: "config.toml"; Flags: ignoreversion recursesubdirs

[Dirs]
; Concedemos permisos de escritura a todos los usuarios para que el agente pueda generar los logs y PDFs temporales
Name: "{app}"; Permissions: users-modify

[Registry]
; Agregar la llave para arrancar con Windows cada vez que el usuario inicie sesión
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "PrintAgentRS"; ValueData: """{app}\print-agent.exe"""; Flags: uninsdeletevalue

[Icons]
Name: "{group}\Agente AIR"; Filename: "{app}\print-agent.exe"
Name: "{group}\Uninstall Agente AIR"; Filename: "{uninstallexe}"

[Run]
; Iniciar automáticamente después de instalar (sin checkbox, siempre arranca)
Filename: "{app}\print-agent.exe"; Flags: nowait runascurrentuser

[Code]
var
  ConfigPage: TInputQueryWizardPage;

procedure InitializeWizard;
var
  ResultCode: Integer;
begin
  // Crear una página interactiva personalizada
  ConfigPage := CreateInputQueryPage(wpSelectDir,
    'Configuración del Agente de Impresión',
    'Por favor ingrese las credenciales de este punto de venta.',
    'El agente necesita esta información para conectarse e identificarse en el servidor MQTT.');

  // Añadir campos (IDs)
  ConfigPage.Add('Ambiente (Ej: Dev, Test, Prod):', False);
  ConfigPage.Add('ID Cliente (Ej: 118):', False);
  ConfigPage.Add('ID Punto (Ej: 285):', False);
  
  // Valores por defecto
  ConfigPage.Values[0] := 'Dev';
  ConfigPage.Values[1] := '';
  ConfigPage.Values[2] := '';

  // Asesinar silenciosamente cualquier instancia rebelde o zombie del agente antes de instalar
  Exec('taskkill.exe', '/F /IM print-agent.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

// Esta función se dispara antes de proceder con pasos críticos de desinstalación
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  ResultCode: Integer;
begin
  if CurUninstallStep = usUninstall then
  begin
    // Asesinar antes de desinstalar para evitar bloqueos
    Exec('taskkill.exe', '/F /IM print-agent.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  end;
end;

// Esta función se dispara cuando el archivo se ha copiado
procedure CurStepChanged(CurStep: TSetupStep);
var
  TomlLines: TArrayOfString;
begin
  if CurStep = ssPostInstall then
  begin
    // Si el config.toml ya existe (ej: actualización silenciosa OTA), lo respetamos absolutamente
    if not FileExists(ExpandConstant('{app}\config.toml')) then
    begin
      // Escribimos el config.toml generado, asegurando minúsculas ya que Rust es estricto (serde lowercase)
      SetArrayLength(TomlLines, 6);
      TomlLines[0] := 'ambiente = "' + Lowercase(ConfigPage.Values[0]) + '"';
      TomlLines[1] := 'id_cliente = "' + Lowercase(ConfigPage.Values[1]) + '"';
      TomlLines[2] := 'id_punto = "' + Lowercase(ConfigPage.Values[2]) + '"';
      TomlLines[3] := '';
      TomlLines[4] := '# Puedes descomentar y editar las lineas maestras si fuese necesario:';
      TomlLines[5] := '# broker_url = "mqtt://..."';
      
      // Guardar el archivo fisico en el disco final de la app
      SaveStringsToFile(ExpandConstant('{app}\config.toml'), TomlLines, False);
    end;
  end;
end;
