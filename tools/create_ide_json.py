#!/usr/bin/env python
# -*- coding: utf-8 -*-

'''
Generate different product packages according to the environment
'''

import os
import platform
import tarfile
import zipfile


def pack_file(file_path, archive_path):
    if not os.path.exists(file_path):
        print(f"Erorr: can't found {file_path}")
        return
    if archive_path.endswith('.zip'):
        with zipfile.ZipFile(archive_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
            zipf.write(file_path, os.path.basename(file_path))
    else:
        with tarfile.open(archive_path, "w:gz") as tar:
            tar.add(file_path, arcname=os.path.basename(file_path),
                    recursive=False)


def tyutool_env():
    # {linux/windows/darwin_{x86/arm64}}
    _env = platform.system().lower()
    if "linux" in _env:
        env = "linux"
    elif "darwin" in _env:
        machine = "x86" if "x86" in platform.machine().lower() else "arm64"
        env = f"darwin_{machine}"
    else:
        env = "windows"
    return env


if __name__ == '__main__':
    env = tyutool_env()
    root_re = os.path.join(__file__, "../..")
    root = os.path.abspath(root_re)
    output_dir = os.path.join(root, "dist")

    # Be consistent with the content of file ->
    # .github/workflows/release.yml
    if env == "windows":
        cli_path = os.path.join(output_dir, "tyutool_cli.exe")
        gui_path = os.path.join(output_dir, "tyutool_gui.exe")
        cli_tar_path = os.path.join(output_dir, "windows_tyutool_cli.zip")
        gui_tar_path = os.path.join(output_dir, "windows_tyutool_gui.zip")
    else:
        cli_path = os.path.join(output_dir, "tyutool_cli")
        gui_path = os.path.join(output_dir, "tyutool_gui")
        cli_tar_path = os.path.join(output_dir, f"{env}_tyutool_cli.tar.gz")
        gui_tar_path = os.path.join(output_dir, f"{env}_tyutool_gui.tar.gz")

    pack_file(cli_path, cli_tar_path)
    pack_file(gui_path, gui_tar_path)

    print('Finished updata files.')
