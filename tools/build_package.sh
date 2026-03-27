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
# On macOS release: don't pass --windowed to PyInstaller (avoids deprecated -F+--windowed combo
# that creates a conflicting .app bundle). Instead, build a plain onefile executable and wrap it
# in a .app bundle manually afterward.
if [ "$(uname -s)" = "Darwin" ] && [ "$DEBUG_MODE" != "1" ]; then
    GUI_WINDOWED=""
else
    GUI_WINDOWED="$WINDOWED"
fi

pyinstaller -F --workpath build --specpath dist $GUI_WINDOWED --icon ./resource/logo.${ICO} --add-data "./resource/libs:./resource/libs" $EXCLUDES -n "tyutool_gui${SUFFIX}" ./tyutool_gui.py

sleep 1

# On macOS, wrap GUI executable in a .app bundle to avoid Terminal window on launch
if [ "$(uname -s)" = "Darwin" ] && [ "$DEBUG_MODE" != "1" ]; then
    APP_NAME="tyutool_gui${SUFFIX}"
    APP_BUNDLE="./dist/${APP_NAME}.app"
    rm -rf "$APP_BUNDLE"
    mkdir -p "$APP_BUNDLE/Contents/MacOS"
    mkdir -p "$APP_BUNDLE/Contents/Resources"
    mv "./dist/${APP_NAME}" "$APP_BUNDLE/Contents/MacOS/${APP_NAME}"
    cp "./resource/logo.icns" "$APP_BUNDLE/Contents/Resources/logo.icns"
    cat > "$APP_BUNDLE/Contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>${APP_NAME}</string>
    <key>CFBundleName</key>
    <string>tyuTool</string>
    <key>CFBundleDisplayName</key>
    <string>tyuTool</string>
    <key>CFBundleIdentifier</key>
    <string>com.tuya.tyutool</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>CFBundleIconFile</key>
    <string>logo</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.13</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSUIElement</key>
    <false/>
</dict>
</plist>
PLIST
    echo "Created macOS app bundle: $APP_BUNDLE"
fi

python ./tools/create_ide_json.py
