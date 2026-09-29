#define AppVersion GetEnv("PERCH_VERSION")
#define TargetArch GetEnv("PERCH_ARCH")
#define AppBinary GetEnv("PERCH_BINARY")
#define OutputPath GetEnv("PERCH_OUTPUT")
#define AppIcon GetEnv("PERCH_ICON")

#if AppVersion == "" || AppBinary == "" || OutputPath == "" || AppIcon == ""
  #error Required packaging environment variable is missing
#endif
#if TargetArch == "x86_64"
  #define WinArch "x64os"
#elif TargetArch == "aarch64"
  #define WinArch "arm64"
#else
  #error Unsupported architecture
#endif

[Setup]
AppId={{F8DA8BAC-92B8-4E3E-BA7F-05C27780D49A}
AppName=Perch
AppVersion={#AppVersion}
AppPublisher=Perch
DefaultDirName={localappdata}\Programs\Perch
DefaultGroupName=Perch
PrivilegesRequired=lowest
ArchitecturesAllowed={#WinArch}
ArchitecturesInstallIn64BitMode={#WinArch}
OutputDir={#OutputPath}
OutputBaseFilename=Perch-{#AppVersion}-windows-{#TargetArch}-setup
SetupIconFile={#AppIcon}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\perch.exe

[Files]
Source: "{#AppBinary}"; DestDir: "{app}"; DestName: "perch.exe"; Flags: ignoreversion

[Icons]
Name: "{group}\Perch"; Filename: "{app}\perch.exe"
Name: "{userdesktop}\Perch"; Filename: "{app}\perch.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"
