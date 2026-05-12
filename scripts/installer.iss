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

[UninstallDelete]
Type: filesandordirs; Name: "{app}\*"
Type: dirifempty; Name: "{app}"

[Icons]
Name: "{group}\Agente AIR"; Filename: "{app}\print-agent.exe"
Name: "{group}\Uninstall Agente AIR"; Filename: "{uninstallexe}"

[Run]
; Iniciar automáticamente el script guardián después de instalar usando el intérprete de Windows
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
  ConfigPage.Add('Ambiente (Ej: Dev, Test, Prod, Prod_GD4):', False);
  ConfigPage.Add('ID Cliente (Ej: 118):', False);
  ConfigPage.Add('ID Punto (Ej: 285):', False);
  
  // Valores por defecto
  ConfigPage.Values[0] := 'Dev';
  ConfigPage.Values[1] := '';
  ConfigPage.Values[2] := '';

  // Desactivar cualquier guardián previo
  if DirExists(ExpandConstant('{localappdata}\PrintAgentRS')) then
  begin
    SaveStringToFile(ExpandConstant('{localappdata}\PrintAgentRS\stop.lock'), 'stop', False);
  end;

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
    // Escribir stop.lock para asegurar que el guardián no lo reviva durante desinstalación
    SaveStringToFile(ExpandConstant('{app}\stop.lock'), 'stop', False);
    // Asesinar antes de desinstalar para evitar bloqueos
    Exec('taskkill.exe', '/F /IM print-agent.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  end;
end;

// Esta función valida que no se dejen campos vacíos si es una instalación limpia
function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  
  // Si estamos en nuestra página personalizada
  if CurPageID = ConfigPage.ID then
  begin
    // Validar SOLO si es una instalación desde cero (no hay config.toml)
    if not FileExists(ExpandConstant('{app}\config.toml')) then
    begin
      if (Trim(ConfigPage.Values[0]) = '') or (Trim(ConfigPage.Values[1]) = '') or (Trim(ConfigPage.Values[2]) = '') then
      begin
        MsgBox('Todos los campos (Ambiente, ID Cliente e ID Punto) son OBLIGATORIOS para una instalación nueva.', mbError, MB_OK);
        Result := False;
      end;
    end;
  end;
end;

// Esta función decide si se debe saltar una página del instalador
function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := False;
  
  // Si es la página de configuración y ya existe un config.toml, la saltamos para no confundir al usuario
  if PageID = ConfigPage.ID then
  begin
    if FileExists(ExpandConstant('{app}\config.toml')) then
    begin
      Result := True;
    end;
  end;
end;

// Esta función se dispara cuando el archivo se ha copiado
procedure CurStepChanged(CurStep: TSetupStep);
var
  TomlLines: TArrayOfString;
  VbsLines: TArrayOfString;
begin
  // Creamos el script en ssInstall para que esté listo ANTES del arranque post-instalación
  if CurStep = ssInstall then
  begin
    // Generar Guardián VBScript con Protección Mutua y Bajo Consumo (Polling)
    SetArrayLength(VbsLines, 17);
    VbsLines[0] := 'Set fso = CreateObject("Scripting.FileSystemObject")';
    VbsLines[1] := 'Set shell = WScript.CreateObject("WScript.Shell")';
    VbsLines[2] := 'strDir = fso.GetParentFolderName(WScript.ScriptFullName)';
    VbsLines[3] := 'shell.CurrentDirectory = strDir';
    VbsLines[4] := '';
    VbsLines[5] := 'On Error Resume Next';
    VbsLines[6] := 'Set lockFile = fso.OpenTextFile("guardian.lock", 2, True)';
    VbsLines[7] := 'If Err.Number <> 0 Then WScript.Quit';
    VbsLines[8] := 'On Error GoTo 0';
    VbsLines[9] := '';
    VbsLines[10] := 'Set objWMIService = GetObject("winmgmts:\\.\root\cimv2")';
    VbsLines[11] := 'Do';
    VbsLines[12] := '  If fso.FileExists("stop.lock") Then Exit Do';
    VbsLines[13] := '  Set colProcesses = objWMIService.ExecQuery("Select * from Win32_Process Where Name = ''print-agent.exe''")';
    VbsLines[14] := '  If colProcesses.Count = 0 Then shell.Run "print-agent.exe --revived", 0, False';
    VbsLines[15] := '  WScript.Sleep 2000';
    VbsLines[16] := 'Loop';
    SaveStringsToFile(ExpandConstant('{app}\lanzador.vbs'), VbsLines, False);
  end;

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
