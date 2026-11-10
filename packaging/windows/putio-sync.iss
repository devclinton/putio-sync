; Windows installer for putio-sync (feature 053), built with Inno Setup 6:
;   iscc /DAppVersion=0.1.2 /DArch=x64 /DSourceExe=path\to\putio-sync.exe packaging\windows\putio-sync.iss
; Arch is x64 or arm64. Writes putio-sync-<version>-windows-<arch>-setup.exe
; next to this script, or under /O<dir>.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\putio-sync.exe"
#endif

[Setup]
AppId={{6F0B8D2A-6C1E-4F57-9C38-5B1D2E7A4C90}
AppName=Put.io Sync
AppVersion={#AppVersion}
AppPublisher=putio-sync contributors
AppPublisherURL=https://github.com/devclinton/putio-sync
AppSupportURL=https://github.com/devclinton/putio-sync/issues
DefaultDirName={autopf}\putio-sync
DefaultGroupName=Put.io Sync
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
OutputBaseFilename=putio-sync-{#AppVersion}-windows-{#Arch}-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=admin
ChangesEnvironment=yes
UninstallDisplayName=Put.io Sync {#AppVersion}
#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif

[Tasks]
Name: "service"; Description: "Run the daemon in the background at startup (a SYSTEM scheduled task; config in {commonappdata}\putio-sync)"; GroupDescription: "Run putio-sync:"; Flags: exclusive
Name: "login"; Description: "Start the daemon when I sign in, in a minimized window (config in %USERPROFILE%\.putiosync)"; GroupDescription: "Run putio-sync:"; Flags: exclusive unchecked
Name: "manual"; Description: "Don't start it; I'll run putio-sync daemon myself"; GroupDescription: "Run putio-sync:"; Flags: exclusive unchecked
Name: "path"; Description: "Add putio-sync to PATH"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "putio-sync.exe"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"
Source: "putio-sync-service.cmd"; DestDir: "{app}"; Flags: ignoreversion

[Dirs]
Name: "{commonappdata}\putio-sync"; Tasks: service

[INI]
Filename: "{app}\Put.io Sync Web UI.url"; Section: "InternetShortcut"; Key: "URL"; String: "http://localhost:15432/"

[Icons]
Name: "{group}\Put.io Sync Web UI"; Filename: "{app}\Put.io Sync Web UI.url"
Name: "{group}\Uninstall Put.io Sync"; Filename: "{uninstallexe}"
Name: "{userstartup}\putio-sync daemon"; Filename: "{app}\putio-sync.exe"; Parameters: "daemon"; WorkingDir: "{app}"; Flags: runminimized; Tasks: login

[Registry]
Root: HKLM; Subkey: "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: path; Check: NeedsAddPath(ExpandConstant('{app}'))

[Run]
Filename: "{sys}\schtasks.exe"; Parameters: "/Create /F /TN ""putio-sync"" /SC ONSTART /RU SYSTEM /RL HIGHEST /TR ""\""{app}\putio-sync-service.cmd\"""""; Flags: runhidden; Tasks: service; StatusMsg: "Creating the putio-sync task..."
Filename: "{sys}\schtasks.exe"; Parameters: "/Run /TN ""putio-sync"""; Flags: runhidden; Tasks: service
Filename: "{app}\putio-sync.exe"; Parameters: "daemon"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent runminimized; Description: "Start the daemon now"; Tasks: login
Filename: "{app}\Put.io Sync Web UI.url"; Flags: shellexec nowait postinstall skipifsilent; Description: "Open the web UI"; Tasks: service login

[UninstallRun]
Filename: "{sys}\schtasks.exe"; Parameters: "/End /TN ""putio-sync"""; Flags: runhidden; RunOnceId: "EndTask"
Filename: "{sys}\schtasks.exe"; Parameters: "/Delete /F /TN ""putio-sync"""; Flags: runhidden; RunOnceId: "DeleteTask"
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM putio-sync.exe"; Flags: runhidden; RunOnceId: "KillDaemon"

[UninstallDelete]
Type: files; Name: "{app}\Put.io Sync Web UI.url"

[Code]
function NeedsAddPath(Dir: string): Boolean;
var
  Path: string;
begin
  if not RegQueryStringValue(HKLM, 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment', 'Path', Path) then
  begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Path) + ';') = 0;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Path, Dir: string;
  P: Integer;
begin
  if CurUninstallStep <> usPostUninstall then
    exit;
  if not RegQueryStringValue(HKLM, 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment', 'Path', Path) then
    exit;
  Dir := ExpandConstant('{app}');
  P := Pos(';' + Uppercase(Dir), Uppercase(Path));
  if P > 0 then
  begin
    Delete(Path, P, Length(Dir) + 1);
    RegWriteExpandStringValue(HKLM, 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment', 'Path', Path);
  end;
end;
