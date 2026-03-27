#!/usr/bin/env bash

cd `dirname $0`
cd ..

# Parse arguments
DEBUG_MODE=0
for arg in "$@"; do
    case "$arg" in
        --debug) DEBUG_MODE=1 ;;
    esac
done

rm -rf ./dist
mkdir -p dist
cp -r ./resource ./dist
cp -r ./tyutool/ai_debug/web/libs/PyOgg/pyogg/libs ./dist/resource

if [ "$(uname -s)" = "Darwin" ]; then
    ICO=icns
else
    ICO=ico
fi

if [ "$DEBUG_MODE" = "1" ]; then
    echo "Building DEBUG version..."
    SUFFIX="_debug"
    WINDOWED=""
else
    echo "Building RELEASE version..."
    SUFFIX=""
    WINDOWED="--windowed"
fi

# Exclude unused heavy modules to reduce bundle size and startup time
EXCLUDES="--exclude-module matplotlib --exclude-module scipy --exclude-module PIL --exclude-module tkinter"
EXCLUDES="$EXCLUDES --exclude-module PySide6.QtWebEngine --exclude-module PySide6.QtWebEngineCore --exclude-module PySide6.QtWebEngineWidgets"
EXCLUDES="$EXCLUDES --exclude-module PySide6.Qt3DCore --exclude-module PySide6.Qt3DRender --exclude-module PySide6.Qt3DInput --exclude-module PySide6.Qt3DLogic --exclude-module PySide6.Qt3DExtras --exclude-module PySide6.Qt3DAnimation"
EXCLUDES="$EXCLUDES --exclude-module PySide6.QtCharts --exclude-module PySide6.QtDataVisualization --exclude-module PySide6.QtGraphs"
EXCLUDES="$EXCLUDES --exclude-module PySide6.QtQuick --exclude-module PySide6.QtQuickWidgets --exclude-module PySide6.QtQml"
EXCLUDES="$EXCLUDES --exclude-module PySide6.QtBluetooth --exclude-module PySide6.QtNfc --exclude-module PySide6.QtPositioning --exclude-module PySide6.QtLocation"
EXCLUDES="$EXCLUDES --exclude-module PySide6.QtPdf --exclude-module PySide6.QtPdfWidgets"
EXCLUDES="$EXCLUDES --exclude-module PySide6.QtRemoteObjects --exclude-module PySide6.QtSensors --exclude-module PySide6.QtSerialPort --exclude-module PySide6.QtTest"
EXCLUDES="$EXCLUDES --exclude-module PySide6.QtWebChannel --exclude-module PySide6.QtWebSockets --exclude-module PySide6.QtSvgWidgets"

pyinstaller -F --workpath build --specpath dist --add-data "./resource/libs:./resource/libs" $EXCLUDES -n "tyutool_cli${SUFFIX}" ./tyutool_cli.py
pyinstaller -F --workpath build --specpath dist $WINDOWED --icon ./resource/logo.${ICO} --add-data "./resource/libs:./resource/libs" $EXCLUDES -n "tyutool_gui${SUFFIX}" ./tyutool_gui.py

sleep 1

python ./tools/create_ide_json.py
