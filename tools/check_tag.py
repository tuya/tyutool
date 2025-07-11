#!/usr/bin/env python3
# coding=utf-8

import os
import sys
import re


def check_tag(version_file, tag_name):
    ver_context = ""
    with open(version_file, encoding='utf-8') as f:
        ver_context = f.read()
    ver_pattern = r'(?<=TYUTOOL_VERSION = ")\d*\.\d*\.\d*(?=\")'
    ret = re.search(ver_pattern, ver_context)
    if not ret:
        print("Search version error.")
        sys.exit(1)
    version = ret.group()

    if version != tag_name:
        print(f"check TAG_NAME error: {version} != {tag_name}.")
        return False
    return True


def main():
    if len(sys.argv) < 2:
        print("error: missing parameter [tag_name].")
        sys.exit(1)

    root_re = os.path.join(__file__, "../..")
    root = os.path.abspath(root_re)
    version_file = os.path.join(root, "tyutool", "util", "util.py")
    tag_name = sys.argv[1].lstrip('v')

    if check_tag(version_file, tag_name):
        sys.exit(0)
    else:
        sys.exit(1)
    pass


if __name__ == "__main__":
    main()
