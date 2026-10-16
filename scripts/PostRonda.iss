; Inno Setup script for the Windows installer. Built by scripts/bundle-windows.ps1, which needs
; the package folder in dist/ and passes the version as /DAppVersion=x.y.z.
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6F3A2B8E-5C1D-4E7A-9B0F-2D8C4A1E7F53}
AppName=Post Ronda
AppVersion={#AppVersion}
AppVerName=Post Ronda {#AppVersion}
AppPublisher=Post Ronda
AppPublisherURL=https://github.com/ahmadarif-lab/post-ronda
DefaultDirName={autopf}\Post Ronda
DefaultGroupName=Post Ronda
DisableProgramGroupPage=yes
; Installs for the current user without asking for administrator rights (a choice is offered).
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=Post-Ronda-Setup-x64
SetupIconFile=..\assets\PostRonda.ico
UninstallDisplayIcon={app}\PostRonda.exe
LicenseFile=..\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; Flags: unchecked

[Files]
Source: "..\dist\Post-Ronda-windows-x64\PostRonda.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\Inter-LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\Tabler-LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Post Ronda"; Filename: "{app}\PostRonda.exe"
Name: "{autodesktop}\Post Ronda"; Filename: "{app}\PostRonda.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\PostRonda.exe"; Description: "{cm:LaunchProgram,Post Ronda}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; The in-app updater keeps the previous program here until the next start.
Type: files; Name: "{app}\PostRonda.exe.old"
