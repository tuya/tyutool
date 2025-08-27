#!/usr/bin/env bash

cd `dirname $0`
cd ..

rm -rf ./dist
mkdir -p dist
cp -r ./resource ./dist
cp -r ./tyutool/ai_debug/web/libs/PyOgg/pyogg/libs ./dist/resource

if [ "$(uname -s)" = "Darwin" ]; then
    ICO=icns
else
    ICO=ico
fi

pyinstaller -F --workpath build --specpath dist --add-data "./resource/libs:./resource/libs" ./tyutool_cli.py
pyinstaller -F --workpath build --specpath dist --windowed --icon ./resource/logo.${ICO} --add-data "./resource/libs:./resource/libs" ./tyutool_gui.py

sleep 1

python ./tools/create_ide_json.py
