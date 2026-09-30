; GameRecorder installer (NSIS). Per-user install, no admin rights needed.
; Built by scripts/build-windows.sh:  makensis -DVERSION=x.y.z -DSRC=<folder with exe+dll> GameRecorder.nsi

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"

!ifndef VERSION
  !define VERSION "0.1.0"
!endif
!ifndef SRC
  !define SRC "..\dist\app"
!endif
!ifndef OUTDIR
  !define OUTDIR "..\dist"
!endif

!define APPNAME "GameRecorder"
!define EXE "GameRecorder.exe"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}"
!define WEBVIEW2_GUID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

Name "${APPNAME}"
OutFile "${OUTDIR}\${APPNAME}-Setup-${VERSION}.exe"
InstallDir "$LOCALAPPDATA\Programs\${APPNAME}"
InstallDirRegKey HKCU "Software\${APPNAME}" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "${APPNAME} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "FileDescription" "${APPNAME} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "MIT License"

!define MUI_ICON "..\app\icons\icon.ico"
!define MUI_UNICON "..\app\icons\icon.ico"
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TITLE "Install ${APPNAME}"
!define MUI_WELCOMEPAGE_TEXT "${APPNAME} records your games automatically with an event timeline.$\r$\n$\r$\nIt uses OBS Studio (free) to record on your graphics card. After installing, the app walks you through connecting OBS.$\r$\n$\r$\nIf ${APPNAME} is already running, quit it from the tray icon first."
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APPNAME}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ; The UI needs Microsoft Edge WebView2 (built into Windows 11 and up-to-date Windows 10).
  ReadRegStr $0 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_GUID}" "pv"
  ${If} $0 == ""
    ReadRegStr $0 HKCU "Software\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_GUID}" "pv"
  ${EndIf}
  ${If} $0 == ""
  ${OrIf} $0 == "0.0.0.0"
    MessageBox MB_YESNO|MB_ICONINFORMATION "${APPNAME} needs the Microsoft Edge WebView2 Runtime, which isn't installed on this PC.$\r$\n$\r$\nOpen the download page now? (Install it, then run this setup again.)" IDNO +2
    ExecShell "open" "https://developer.microsoft.com/microsoft-edge/webview2/#download"
    Abort
  ${EndIf}
FunctionEnd

Section "Install"
  SetOutPath "$INSTDIR"
  SetOverwrite on
  File "${SRC}\${EXE}"
  File "${SRC}\WebView2Loader.dll"
  File "..\LICENSE"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\${APPNAME}" "InstallDir" "$INSTDIR"

  CreateDirectory "$SMPROGRAMS"
  CreateShortCut "$SMPROGRAMS\${APPNAME}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0
  CreateShortCut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0

  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "${APPNAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\${EXE}"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  ; Your recordings, clips and settings are kept.
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\WebView2Loader.dll"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APPNAME}.lnk"
  Delete "$DESKTOP\${APPNAME}.lnk"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APPNAME}"
  DeleteRegKey HKCU "${UNINSTKEY}"
  DeleteRegKey HKCU "Software\${APPNAME}"
SectionEnd
