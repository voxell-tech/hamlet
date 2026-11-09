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
; An update Hamlet ran itself, silently, opens Hamlet again.
Filename: "{app}\hamlet_launch.exe"; Flags: nowait; Check: Relaunch

[UninstallRun]
; The server and the agents' commands hold the programs open.
Filename: "{app}\hamlet.exe"; Parameters: "stop"; Flags: runhidden waituntilterminated; RunOnceId: "StopServer"

[Code]
function Relaunch: Boolean;
begin
  Result := ExpandConstant('{param:RELAUNCH|no}') = 'yes';
end;

// A running server holds its programs and Node open, so an update
// stops it first, and does not go on while it serves. Its agents
// carry on once Hamlet is opened again.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Hamlet: String;
  Code: Integer;
begin
  Result := '';
  Hamlet := ExpandConstant('{app}\hamlet.exe');
  if not FileExists(Hamlet) then
    Exit;
  if not Exec(Hamlet, 'stop', '', SW_HIDE, ewWaitUntilTerminated, Code) then
    Result := 'Setup could not run "' + Hamlet + ' stop": '
      + SysErrorMessage(Code) + '. Close Hamlet and try again.'
  else if Code <> 0 then
    Result := 'Hamlet''s server did not stop ("hamlet stop" exited with '
      + IntToStr(Code) + '). Run "hamlet stop" in a terminal to see '
      + 'why, then try again.';
end;
