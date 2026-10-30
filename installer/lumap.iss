; lumap - Windows installer (Inno Setup 6).
; Built by installer\build.ps1: ISCC /DAppVersion=<x.y.z> /DSource=<dist folder> lumap.iss
;
; Per-user install (no administrator prompt) into %LocalAppData%\Programs\lumap, with the
; bundled ffmpeg and hand models beside lumap.exe (where the app looks for them).
; Settings live in %APPDATA%\lumap; the uninstaller keeps them unless asked.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Source
  #define Source "..\dist"
#endif

#define AppName "lumap"
#define AppExe "lumap.exe"

[Setup]
; The AppId never changes: it is how an upgrade finds the installation it replaces.
AppId={{7C1F0E8A-5B2D-4C33-9E61-2A7D4F0B8E55}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=REX Technologies
AppPublisherURL=https://tochi-mba.github.io/lumap/
AppSupportURL=https://github.com/tochi-mba/lumap/issues
AppUpdatesURL=https://github.com/tochi-mba/lumap/releases/latest
AppComments=Projection mapping you can trust on show day.
AppContact=https://github.com/tochi-mba/lumap/issues
VersionInfoDescription={#AppName} setup
VersionInfoProductName={#AppName}
VersionInfoCompany=REX Technologies
; A second installer would fight the first over the same files.
SetupMutex=LumapSetup
DefaultDirName={localappdata}\Programs\{#AppName}
DisableDirPage=yes
DisableProgramGroupPage=yes
DefaultGroupName={#AppName}
PrivilegesRequired=lowest
OutputBaseFilename=lumap-Setup-{#AppVersion}
SetupIconFile=..\assets\lumap.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
Compression=lzma2/ultra64
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
CloseApplications=yes
RestartApplications=no
ChangesEnvironment=yes
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
SelectTasksLabel2=Choose how {#AppName} should be set up, then click Next.%n%nIt installs for you only, without administrator rights, and brings its own ffmpeg for video.
FinishedHeadingLabel=Ready for the wall
FinishedLabelNoIcons={#AppName} is installed. Plug in the projectors and click Create outputs.
FinishedLabel={#AppName} is installed. Plug in the projectors and click Create outputs.
ConfirmUninstall=Remove {#AppName} from this PC?%n%nYour projects and media are not touched.

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "After installing:"; Flags: checkedonce
Name: "addtopath"; Description: "Add the lumap command to my PATH"; GroupDescription: "After installing:"; Flags: checkedonce
Name: "associate"; Description: "Open .lumap.toml projects with lumap"; GroupDescription: "After installing:"; Flags: checkedonce

[Files]
Source: "prepare-upgrade.ps1"; Flags: dontcopy
Source: "{#Source}\lumap\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\lumap\ffmpeg\*"; DestDir: "{app}\ffmpeg"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Source}\lumap\models\*"; DestDir: "{app}\models"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Source}\lumap\examples\*"; DestDir: "{app}\examples"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Source}\lumap\docs\*"; DestDir: "{app}\docs"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Source}\lumap\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\lumap\CHANGELOG.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\lumap\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\lumap\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\{#AppExe}"
Name: "{autoprograms}\{#AppName} - New show"; Filename: "{app}\{#AppExe}"; Parameters: "--new-show"; IconFilename: "{app}\{#AppExe}"; Comment: "Create a blank projection mapping show"
Name: "{autoprograms}\{#AppName} - Open show"; Filename: "{app}\{#AppExe}"; Parameters: "--open-show"; IconFilename: "{app}\{#AppExe}"; Comment: "Open an existing projection mapping show"
Name: "{autoprograms}\{#AppName} - Check for updates"; Filename: "{app}\{#AppExe}"; Parameters: "--check-updates"; IconFilename: "{app}\{#AppExe}"; Comment: "Check for verified lumap releases"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\{#AppExe}"; Tasks: desktopicon

; .lumap.toml projects open in lumap when asked for. Everything is per user and offered, never
; imposed: lumap joins Explorer's "Open with", and the type's existing default is left alone
; unless the task is ticked.
[Registry]
Root: HKA; Subkey: "Software\Classes\lumap.project"; ValueType: string; ValueData: "lumap project"; Flags: uninsdeletekey; Tasks: associate
Root: HKA; Subkey: "Software\Classes\lumap.project"; ValueType: string; ValueName: "FriendlyTypeName"; ValueData: "lumap project"; Tasks: associate
Root: HKA; Subkey: "Software\Classes\lumap.project\DefaultIcon"; ValueType: string; ValueData: "{app}\{#AppExe},0"; Tasks: associate
Root: HKA; Subkey: "Software\Classes\lumap.project\shell\open\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.toml\OpenWithProgids"; ValueType: string; ValueName: "lumap.project"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "lumap"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\DefaultIcon"; ValueType: string; ValueData: "{app}\{#AppExe},0"
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\shell\open\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".toml"; ValueData: ""

[Run]
Filename: "{app}\{#AppExe}"; Description: "Open {#AppName}"; Flags: nowait postinstall skipifsilent
; An update started from inside lumap installs silently, then opens it again.
Filename: "{app}\{#AppExe}"; Flags: nowait; Check: RelaunchAfterUpdate

[Code]
const
  EnvironmentKey = 'Environment';

// lumap passes /relaunch=1 when it installs an update of itself.
function RelaunchAfterUpdate: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:relaunch|0}') = '1');
end;

// Stop the installed copy before its files are replaced.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Arguments: String;
  ResultCode: Integer;
begin
  Result := '';
  ExtractTemporaryFile('prepare-upgrade.ps1');
  Arguments := ExpandConstant('-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "{tmp}\prepare-upgrade.ps1" -InstallDirectory "{app}"');
  if not Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    Arguments, '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
    Result := 'Could not start upgrade preparation. Close lumap and retry.';
  if (Result = '') and (ResultCode <> 0) then
    Result := 'Could not close the installed lumap. Close it and retry. No files have been replaced.';
end;

procedure EnvAddPath(Path: String);
var
  Paths: String;
begin
  if not RegQueryStringValue(HKCU, EnvironmentKey, 'Path', Paths) then
    Paths := '';
  if Pos(';' + Uppercase(Path) + ';', ';' + Uppercase(Paths) + ';') > 0 then
    exit;
  if (Paths <> '') and (Paths[Length(Paths)] <> ';') then
    Paths := Paths + ';';
  RegWriteExpandStringValue(HKCU, EnvironmentKey, 'Path', Paths + Path);
end;

procedure EnvRemovePath(Path: String);
var
  Paths: String;
  P: Integer;
begin
  if not RegQueryStringValue(HKCU, EnvironmentKey, 'Path', Paths) then
    exit;
  P := Pos(';' + Uppercase(Path) + ';', ';' + Uppercase(Paths) + ';');
  if P = 0 then
    exit;
  Delete(Paths, P - 1, Length(Path) + 1);
  RegWriteExpandStringValue(HKCU, EnvironmentKey, 'Path', Paths);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    EnvAddPath(ExpandConstant('{app}'));
end;

// Settings and recent files outlive the app on purpose, so removing them is asked for rather
// than assumed, and never happens during a silent uninstall.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  SettingsDirectory: String;
begin
  if CurUninstallStep <> usPostUninstall then
    exit;

  EnvRemovePath(ExpandConstant('{app}'));
  SettingsDirectory := ExpandConstant('{userappdata}\{#AppName}');
  if not DirExists(SettingsDirectory) then
    exit;

  if UninstallSilent then
    exit;

  if MsgBox('Also delete your lumap settings and recent-file list?' + #13#10 + #13#10 +
      SettingsDirectory + #13#10 + #13#10 +
      'Keep them if you plan to install {#AppName} again. Projects are never touched.',
      mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
    DelTree(SettingsDirectory, True, True, True);
end;
