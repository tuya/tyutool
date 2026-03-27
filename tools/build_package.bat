@echo off

REM Change to the script's directory
cd %~dp0
cd ..

REM Clean and create the dist directory
if exist dist ( rmdir /s /q dist )
mkdir dist

REM Copy resources
xcopy resource dist\resource /E /I /Y
xcopy tyutool\ai_debug\web\libs\PyOgg\pyogg\libs dist\resource\libs /E /I /Y

REM Exclude unused heavy modules to reduce bundle size and startup time
set EXCLUDES=--exclude-module matplotlib --exclude-module scipy --exclude-module PIL --exclude-module tkinter
set EXCLUDES=%EXCLUDES% --exclude-module PySide6.QtWebEngine --exclude-module PySide6.QtWebEngineCore --exclude-module PySide6.QtWebEngineWidgets
set EXCLUDES=%EXCLUDES% --exclude-module PySide6.Qt3DCore --exclude-module PySide6.Qt3DRender --exclude-module PySide6.Qt3DInput --exclude-module PySide6.Qt3DLogic --exclude-module PySide6.Qt3DExtras --exclude-module PySide6.Qt3DAnimation
set EXCLUDES=%EXCLUDES% --exclude-module PySide6.QtCharts --exclude-module PySide6.QtDataVisualization --exclude-module PySide6.QtGraphs
set EXCLUDES=%EXCLUDES% --exclude-module PySide6.QtQuick --exclude-module PySide6.QtQuickWidgets --exclude-module PySide6.QtQml
set EXCLUDES=%EXCLUDES% --exclude-module PySide6.QtBluetooth --exclude-module PySide6.QtNfc --exclude-module PySide6.QtPositioning --exclude-module PySide6.QtLocation
set EXCLUDES=%EXCLUDES% --exclude-module PySide6.QtPdf --exclude-module PySide6.QtPdfWidgets
set EXCLUDES=%EXCLUDES% --exclude-module PySide6.QtRemoteObjects --exclude-module PySide6.QtSensors --exclude-module PySide6.QtSerialPort --exclude-module PySide6.QtTest
set EXCLUDES=%EXCLUDES% --exclude-module PySide6.QtWebChannel --exclude-module PySide6.QtWebSockets --exclude-module PySide6.QtSvgWidgets

REM Build the executables
pyinstaller -F --workpath build --specpath dist --add-data "./resource/libs;./resource/libs" %EXCLUDES% -n tyutool_cli ./tyutool_cli.py
pyinstaller -F --workpath build --specpath dist --windowed --icon ./resource/logo.ico --add-data "./resource/libs;./resource/libs" %EXCLUDES% -n tyutool_gui ./tyutool_gui.py

REM Generate IDE JSON file
python ./tools/create_ide_json.py

echo Build finished.

