[Setup]
AppName=PrintAgent RS
AppVersion=1.0.0
DefaultDirName={autopf}\PrintAgentRS
DefaultGroupName=PrintAgent RS
OutputDir=..\dist
OutputBaseFilename=PrintAgentRS_Installer
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
PrivilegesRequired=admin
DisableProgramGroupPage=yes

[Files]
Source: "..\dist\PrintAgentRS\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs

[Dirs]
; Concedemos permisos de escritura a todos los usuarios para que el agente pueda generar los logs y PDFs temporales
Name: "{app}"; Permissions: users-modify

[Registry]
; Agregar la llave para arrancar con Windows cada vez que el usuario inicie sesión
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "PrintAgentRS"; ValueData: """{app}\print-agent.exe"""; Flags: uninsdeletevalue

[Icons]
Name: "{group}\PrintAgent RS"; Filename: "{app}\print-agent.exe"
Name: "{group}\Uninstall PrintAgent RS"; Filename: "{uninstallexe}"

[Run]
; Iniciar automáticamente después de instalar
Filename: "{app}\print-agent.exe"; Description: "Iniciar PrintAgent RS ahora"; Flags: nowait postinstall runascurrentuser

[Code]
var
  ConfigPage: TInputQueryWizardPage;

procedure InitializeWizard;
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
end;

// Esta función se dispara cuando el archivo se ha copiado
procedure CurStepChanged(CurStep: TSetupStep);
var
  TomlLines: TArrayOfString;
begin
  if CurStep = ssPostInstall then
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
