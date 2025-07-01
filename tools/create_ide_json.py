#!/usr/bin/env python
# -*- coding: utf-8 -*-

'''
在生成可执行文件后，如果需要更新IDE插件中的cli工具，
使用此脚本生成json文件，详情见README.md
'''

import os
import sys
import platform
import hashlib
import json
import tarfile
import re


def create_md5_file(file_name):
    md5_hash = hashlib.md5()

    a_file = open(file_name, "rb")
    content = a_file.read()
    md5_hash.update(content)
    a_file.close()
    hash_data = md5_hash.hexdigest()
    return hash_data


def md5sum(fname):
    if not os.path.isfile(fname):
        return False
    f = open(fname, 'rb')
    m = hashlib.md5()
    # 大文件处理
    while True:
        d = f.read(8096)
        if not d:
            break
        m.update(d)
    ret = m.hexdigest()
    f.close()
    return ret


def gen_json(tar_path, json_path):
    if not os.path.exists(tar_path):
        print(f'cli not found: {tar_path}')
        return False
    hash_data = md5sum(tar_path)
    url = "https://images.tuyacn.com/smart/embed/package/vscode/\
data/ide_serial/tyutool_cli.tar.gz"
    json_data = {'tyutool_cli': {
        'full_name': "tyutool_cli.tar.gz",
        'md5': hash_data,
        'version': "",
        'url': url
    }}
    json_str = json.dumps(json_data, indent=4, ensure_ascii=False)
    with open(json_path, 'w', encoding='utf-8') as f:
        f.write(json_str)
    pass


def gen_upgrade_json(temp_file, version_file, out_file):
    ver_context = ""
    with open(version_file, encoding='utf-8') as f:
        ver_context = f.read()
    ver_pattern = r'(?<=TYUTOOL_VERSION = ")\d*\.\d*\.\d*(?=\")'
    ret = re.search(ver_pattern, ver_context)
    if not ret:
        print("Search version error.")
        sys.exit(1)
    version = ret.group()

    f = open(temp_file, "r", encoding='utf-8')
    temp_data = json.load(f)
    f.close()
    temp_data['version'] = version

    out_str = json.dumps(temp_data, indent=4, ensure_ascii=False)
    with open(out_file, "w") as f:
        f.write(out_str)
    pass


def pack_file(file_path, tar_path):
    if not os.path.exists(file_path):
        print(f"Erorr: can't found {file_path}")
        return
    tar = tarfile.open(tar_path, "w:gz")
    tar.add(file_path, arcname=os.path.basename(file_path), recursive=False)
    tar.close()
    pass


def tyutool_env():
    # {linux/windows/darwin_{x86/arm64}}
    _env = platform.system().lower()
    if "linux" in _env:
        env = "linux"
    elif "darwin" in _env:
        print(f">>>>>>>debug: {platform.machine()}")
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

    if env == "linux":
        cli_path = os.path.join(output_dir, "tyutool_cli")  # 根据可执行文件位置修改
        gui_path = os.path.join(output_dir, "tyutool_gui")
        # 名称不要修改和IDE保持一致
        cli_tar_path = os.path.join(output_dir, "tyutool_cli.tar.gz")
        gui_tar_path = os.path.join(output_dir, "tyutool_gui.tar.gz")
    elif env == "windows":
        cli_path = os.path.join(output_dir, "tyutool_cli.exe")
        gui_path = os.path.join(output_dir, "tyutool_gui.exe")
        cli_tar_path = os.path.join(output_dir, "win_tyutool_cli.tar.gz")
        gui_tar_path = os.path.join(output_dir, "win_tyutool_gui.tar.gz")
    else:
        cli_path = os.path.join(output_dir, "tyutool_cli")
        gui_path = os.path.join(output_dir, "tyutool_gui")
        cli_tar_path = os.path.join(output_dir, f"{env}_tyutool_cli.tar.gz")
        gui_tar_path = os.path.join(output_dir, f"{env}_tyutool_gui.tar.gz")

    pack_file(cli_path, cli_tar_path)
    pack_file(gui_path, gui_tar_path)

    if env == "linux":
        json_path = os.path.join(output_dir, "ideserial.json")  # 不能修改
        gen_json(cli_tar_path, json_path)
        temp_file = os.path.join(root, "tools", "upgrade_config.json")
        version_file = os.path.join(root, "tyutool", "util", "util.py")
        out_file = os.path.join(output_dir, "upgrade_config.json")
        gen_upgrade_json(temp_file, version_file, out_file)

    print('Finished updata files.')
