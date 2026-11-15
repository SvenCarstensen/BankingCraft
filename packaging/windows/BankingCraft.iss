#ifndef AppVersion
  #error AppVersion must be supplied by the release workflow
#endif
#ifndef PayloadDir
  #error PayloadDir must be supplied by the release workflow
#endif

[Setup]
AppId={{C49550E3-5334-4E6D-A986-CEEF2AF68C64}
AppName=BankingCraft
AppVersion={#AppVersion}
AppPublisher=AppCreation GmbH
AppPublisherURL=https://github.com/SvenCarstensen/BankingCraft
DefaultDirName={localappdata}\Programs\BankingCraft
DefaultGroupName=BankingCraft
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputBaseFilename=BankingCraft-{#AppVersion}-windows-x86_64-setup
SetupIconFile={#PayloadDir}\BankingCraft.ico
UninstallDisplayIcon={app}\BankingCraft.ico
LicenseFile={#PayloadDir}\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#PayloadDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\BankingCraft"; Filename: "{app}\BankingCraft.exe"; IconFilename: "{app}\BankingCraft.ico"
Name: "{autodesktop}\BankingCraft"; Filename: "{app}\BankingCraft.exe"; IconFilename: "{app}\BankingCraft.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\BankingCraft.exe"; Description: "{cm:LaunchProgram,BankingCraft}"; Flags: nowait postinstall skipifsilent
