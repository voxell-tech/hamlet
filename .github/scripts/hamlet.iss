; Hamlet's installer for Windows, compiled by Inno Setup 6 from what
; package.sh staged. It installs for the account running it, so it
; asks for no administrator, and adds Hamlet to the Start menu.
;
; package.sh defines Version, Stage (the staged folder) and Icon.

[Setup]
; The installer's identity across versions: never change it, or an
; update installs beside the old one.
AppId={{D75DEEC3-5985-4793-AD98-2E2D59CAFE80}
AppName=Hamlet
AppVersion={#Version}
AppPublisher=Voxell
AppPublisherURL=https://github.com/voxell-tech/hamlet
AppSupportURL=https://github.com/voxell-tech/hamlet/issues
DefaultDirName={localappdata}\Programs\Hamlet
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile={#Icon}
UninstallDisplayIcon={app}\hamlet.ico
UninstallDisplayName=Hamlet
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes

[Tasks]
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Stage}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[InstallDelete]
; An update replaces pi whole, so no module an older pi had is left.
Type: filesandordirs; Name: "{app}\pi"

[Icons]
Name: "{autoprograms}\Hamlet"; Filename: "{app}\hamlet_launch.exe"; IconFilename: "{app}\hamlet.ico"
Name: "{autodesktop}\Hamlet"; Filename: "{app}\hamlet_launch.exe"; IconFilename: "{app}\hamlet.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\hamlet_launch.exe"; Description: "{cm:LaunchProgram,Hamlet}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; The server and the agents' commands hold the programs open.
Filename: "{app}\hamlet.exe"; Parameters: "stop"; Flags: runhidden waituntilterminated; RunOnceId: "StopServer"

[Code]
// A running server holds its programs and Node open, so an update
// stops it first. Its agents carry on once Hamlet is opened again.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Result := '';
  if FileExists(ExpandConstant('{app}\hamlet.exe')) then
    Exec(ExpandConstant('{app}\hamlet.exe'), 'stop', '', SW_HIDE,
      ewWaitUntilTerminated, Code);
end;
