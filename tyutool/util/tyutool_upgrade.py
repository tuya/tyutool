#!/usr/bin/env python3
# -*- coding: utf-8 -*-

import os
import sys
import shutil
import tarfile
import zipfile
import subprocess
import requests
import json
from typing import Callable
from tyutool.util.util import TYUTOOL_ENV, TYUTOOL_ROOT
from tyutool.util.util import (
    tyutool_version, get_country_code, network_available
)


class TyutoolUpgrade(object):
    def __init__(self, logger, running_type,
                 show_progress=None, exit_callback=None):
        self.logger = logger
        self.running_type = "tyutool_" + running_type  # "cli" or "gui"
        self.running_env = TYUTOOL_ENV  # linux/windows/darwin_x86/darwin_arm64

        if self.running_env == "windows":
            file_suffix = ".exe"
            target = f"{self.running_env}_{self.running_type}.zip"
        else:
            file_suffix = ""
            target = f"{self.running_env}_{self.running_type}.tar.gz"

        self.is_script = True if '.py' in sys.argv[0] else False
        self.download_path = os.path.join(TYUTOOL_ROOT, "cache")
        self.download_file = os.path.join(self.download_path, target)

        self.new_file = os.path.join(self.download_path, self.running_type) + \
            file_suffix
        self.old_file = os.path.join(TYUTOOL_ROOT, self.running_type) + \
            file_suffix

        country = get_country_code()

        if country == "China":
            self.github_api_url = "https://gitee.com/api/v5/repos/\
tuya-open/tyutool/releases/latest"
        else:
            self.github_api_url = "https://api.github.com/repos/\
tuya/tyutool/releases/latest"

        if show_progress:
            self.show_progress = show_progress
        else:
            self.show_progress = self._show_progress

        if exit_callback:
            self.exit_callback = exit_callback
        else:
            self.exit_callback = self._default_exit

        self.logger.debug(f"running_type: {self.running_type}")
        self.logger.debug(f"running_env: {self.running_env}")
        self.logger.debug(f"is_script: {self.is_script}")
        self.logger.debug(f"download_file: {self.download_file}")
        self.logger.debug(f"new_file: {self.new_file}")
        self.logger.debug(f"old_file: {self.old_file}")
        self.logger.debug(f"github_api_url: {self.github_api_url}")
        pass

    def _show_progress(self, block_num, block_size, total_size):
        '''CLI progress'''
        downloaded = block_num * block_size
        if total_size > 0:
            progress = min(100, (downloaded / total_size) * 100)
        else:
            progress = 0
        print(f"\rprogress: {progress:.1f}%", end="", flush=True)
        pass

    def _default_exit(self):
        '''Default exit implementation for CLI'''
        sys.exit(0)

    def _download_by_url(self, download_url):
        # use requests download and set timeout
        self.logger.debug(f"download_url: {download_url}")
        try:
            response = requests.get(download_url, stream=True, timeout=5)
            response.raise_for_status()

            total_size = int(response.headers.get('content-length', 0))
            downloaded = 0

            with open(self.download_file, 'wb') as f:
                for chunk in response.iter_content(chunk_size=8192):
                    if chunk:
                        f.write(chunk)
                        downloaded += len(chunk)
                        block_num = downloaded // 8192
                        self.show_progress(block_num, 8192, total_size)

            self.logger.debug("download success")
        except requests.exceptions.Timeout:
            self.logger.error("Download timeout.")
            return False
        except requests.exceptions.RequestException as e:
            self.logger.error(f"Download failed: {e}.")
            return False
        return True

    def _download(self):
        # {linux/windows/darwin_{x86/arm64}}_{cli/gui}
        target = os.path.basename(self.download_file)
        current_version = tyutool_version()
        self.logger.debug(f"target: {target}")
        self.logger.debug(f"current_version: {current_version}")

        try:
            response = requests.get(self.github_api_url, timeout=2)
            data = response.json()
        except requests.exceptions.Timeout:
            self.logger.error("Get version timeout.")
            return False
        except Exception as e:
            self.logger.error(f"Get version error: {e}")
            return False

        server_version = data.get('tag_name', "").lstrip('v')
        assets = data.get('assets', [])

        self.logger.debug(f"server_version: {server_version}")
        assets_list = [asset['name'] for asset in assets]
        self.logger.debug(f"available assets: {assets_list}")

        if not server_version:
            self.logger.error("No version found in GitHub release")
            return False

        if server_version <= current_version:
            self.logger.info(f"[{current_version}] is last version.")
            return False

        # Find matching asset
        download_url = None
        for asset in assets:
            if target == asset['name']:
                download_url = asset['browser_download_url']
                break
        else:
            self.logger.error(f"No matching asset found for {target}")
            return False

        shutil.rmtree(self.download_file, ignore_errors=True)
        shutil.rmtree(self.new_file, ignore_errors=True)
        os.makedirs(self.download_path, exist_ok=True)

        self._download_by_url(download_url)

        if self.download_file.endswith('.zip'):
            with zipfile.ZipFile(self.download_file, 'r') as zip_file:
                zip_file.extractall(path=self.download_path)
        else:
            with tarfile.open(self.download_file, 'r:gz') as tar:
                tar.extractall(path=self.download_path)

        if not os.path.exists(self.new_file):
            return False
        return True

    def _linux_do(self):
        script_path = os.path.join(TYUTOOL_ROOT, "cache", "upgrade.sh")
        script_str = f'''
NEW="{self.new_file}"
OLD="{self.old_file}"
if [ -e "${{OLD}}" ]; then
    sleep 1
    rm -rf ${{OLD}}
fi
cp -rf "${{NEW}}" "${{OLD}}"
echo "Upgrade finish, please restart..."
'''
        with open(script_path, 'w') as f:
            f.write(script_str)
        subprocess.Popen(['bash', script_path])
        pass

    def _windows_do(self):
        script_path = os.path.join(TYUTOOL_ROOT, "cache", "upgrade.bat")
        script_str = f'''
@echo off
set NEW="{self.new_file}"
set OLD="{self.old_file}"
if exist %OLD% (
    timeout /t 1
    del %OLD%
)
copy %NEW% %OLD% >null
echo "Upgrade finish, please restart..."
pause
'''
        with open(script_path, 'w') as f:
            f.write(script_str)
        subprocess.Popen([script_path])
        pass

    def upgrade(self):
        self.logger.info("Upgrading...")
        if self.is_script:
            self.logger.info("Please running [git pull].")
            return

        if not network_available():
            self.logger.error("Network error.")
            return False

        if not self._download():
            return False

        if self.running_env == "windows":
            self._windows_do()
        else:
            self._linux_do()

        self.exit_callback()
        return True

    def ask_upgrade(self, ask: Callable[[str], bool], auto_upgrade=True):
        if self.is_script:
            self.logger.debug("script doesn't need to ask for upgrade.")
            return False

        if not network_available():
            return False

        try:
            response = requests.get(self.github_api_url, timeout=1)
            data = response.json()
        except requests.exceptions.Timeout:
            self.logger.debug("Get version timeout.")
            return False
        except Exception as e:
            self.logger.debug(f"Error: {e}")
            return False

        server_version = data.get('tag_name', "").lstrip('v')
        self.logger.debug(f"server_version: {server_version}")

        skip_version_file = os.path.join(TYUTOOL_ROOT, "cache",
                                         "skip_version.cache")
        skip_version = "0.0.0"
        if os.path.exists(skip_version_file):
            f = open(skip_version_file, 'r', encoding='utf-8')
            json_data = json.load(f)
            skip_version = json_data.get("version", "0.0.0")
            f.close()
        self.logger.debug(f"skip_version: {skip_version}")
        if skip_version >= server_version:
            self.logger.debug(f"skip_version[{skip_version}] >= \
server_version[{server_version}].")
            return False

        current_version = tyutool_version()
        self.logger.debug(f"current_version: {current_version}")
        if server_version <= current_version:
            self.logger.debug(f"[{current_version}] is last version.")
            return False

        ask_ans = ask(server_version)  # 0-yes 1-no 2-skip

        if ask_ans == 1:
            return False
        elif ask_ans == 2:
            skip_version_data = {"version": server_version}
            os.makedirs(os.path.join(TYUTOOL_ROOT, "cache"), exist_ok=True)
            json_str = json.dumps(skip_version_data,
                                  indent=4, ensure_ascii=False)
            f = open(skip_version_file, 'w', encoding='utf-8')
            f.write(json_str)
            f.close()
            return False
        elif ask_ans == 0:
            if not auto_upgrade:
                return True  # gui

        self.upgrade()  # cli
        return False
