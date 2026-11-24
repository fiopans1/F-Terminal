; Generated from scripts/windows/f-terminal.iss by package-windows.ps1.
; The @PLACEHOLDER@ values are replaced at build time.

#define MyAppName "F-Terminal"
#define MyAppVersion "@APP_VERSION@"
#define MyAppPublisher "fiopans1"
#define MyAppURL "https://github.com/fiopans1/F-Terminal"
#define MyAppExeName "f-terminal.exe"
#define MyAppId "{{6B9C54BE-6992-45FA-BD2D-D753D809F61B}"

[Setup]
AppId={#MyAppId}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
AppCopyright=Copyright (C) fiopans1. Licensed under AGPL-3.0.
DefaultDirName={autopf}\F-Terminal
DefaultGroupName=F-Terminal
DisableProgramGroupPage=yes
AllowNoIcons=yes
LicenseFile=@REPO_ROOT@\LICENSE
OutputDir=@OUTPUT_DIR@
OutputBaseFilename=F-Terminal-Setup-@APP_VERSION@-x64
; Filled only when an Authenticode certificate is configured.
@SIGNTOOL_DIRECTIVE@
SetupIconFile=@REPO_ROOT@\assets\windows\f-terminal.ico
UninstallDisplayIcon={app}\f-terminal.ico
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
PrivilegesRequired=admin
CloseApplications=yes
RestartApplications=no
UsePreviousAppDir=yes
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription={#MyAppName} installer
VersionInfoProductName={#MyAppName}
VersionInfoProductVersion={#MyAppVersion}
VersionInfoVersion=@VERSION_INFO@

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: checkablealone

[Files]
Source: "@SOURCE_EXE@"; DestDir: "{app}"; Flags: ignoreversion
Source: "@REPO_ROOT@\assets\windows\f-terminal.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "@REPO_ROOT@\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "@REPO_ROOT@\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\F-Terminal"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\f-terminal.ico"; AppUserModelID: "com.fiopans1.f-terminal"
Name: "{autodesktop}\F-Terminal"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\f-terminal.ico"; AppUserModelID: "com.fiopans1.f-terminal"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent runasoriginaluser
