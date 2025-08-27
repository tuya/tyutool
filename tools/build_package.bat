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

REM Build the executables
pyinstaller -F --workpath build --specpath dist --add-data "./resource/libs;./resource/libs" ./tyutool_cli.py
pyinstaller -F --workpath build --specpath dist --windowed --icon ./resource/logo.ico --add-data "./resource/libs;./resource/libs" ./tyutool_gui.py

REM Generate IDE JSON file
python ./tools/create_ide_json.py

echo Build finished.

