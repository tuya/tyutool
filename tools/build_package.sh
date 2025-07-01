#!/usr/bin/env bash

cd `dirname $0`
cd ..

rm -rf ./dist
mkdir -p dist
cp -a ./resource ./dist

if [ "$(uname -s)" = "Darwin" ]; then
    ICO=icns
else
    ICO=ico
fi

pyinstaller -F --workpath build --specpath dist ./tyutool_cli.py
pyinstaller -F --workpath build --specpath dist --windowed --icon ./resource/logo.${ICO} ./tyutool_gui.py

sleep 1

python ./tools/create_ide_json.py
