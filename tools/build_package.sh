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

pyinstaller -F --workpath build --specpath dist --add-data "./resource/libs:./resource/libs" -n "tyutool_cli${SUFFIX}" ./tyutool_cli.py
pyinstaller -F --workpath build --specpath dist $WINDOWED --icon ./resource/logo.${ICO} --add-data "./resource/libs:./resource/libs" -n "tyutool_gui${SUFFIX}" ./tyutool_gui.py

sleep 1

python ./tools/create_ide_json.py
