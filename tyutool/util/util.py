#!/usr/bin/env python
# -*- coding: utf-8 -*-

import os
import sys
import requests
import logging
import click
import platform
import string
import socket

TYUTOOL_ROOT = os.path.dirname(os.path.abspath(sys.argv[0]))
TYUTOOL_VERSION = "2.0.4"


def tyutool_env():
    _env = platform.system().lower()
    if "linux" in _env:
        env = "linux"
    elif "darwin" in _env:
        machine = "x86" if "x86" in platform.machine().lower() else "arm64"
        env = f"darwin_{machine}"
    else:
        env = "windows"
    return env


TYUTOOL_ENV = tyutool_env()


def tyutool_version():
    return TYUTOOL_VERSION


def tyutool_root():
    return os.getcwd()


TYUT_LOGGER = None
TYUT_LOGGER_H = None


def set_logger(level=logging.WARNING, handler=None):
    global TYUT_LOGGER
    global TYUT_LOGGER_H
    LOG_FORMAT = "[%(levelname)s]: %(message)s"
    if level == logging.DEBUG:
        LOG_FORMAT = "[%(levelname)s][%(filename)s:%(lineno)d]: %(message)s"
    lf = logging.Formatter(fmt=LOG_FORMAT)

    # GUI有更换级别的情况
    if TYUT_LOGGER:
        TYUT_LOGGER.setLevel(level)
        TYUT_LOGGER_H.setFormatter(lf)
        return TYUT_LOGGER

    logger = logging.getLogger("tyut_logger")
    logger.setLevel(level)
    # 输出重定向需要和GUI中使用的一致
    # 使用stderr的好处，可以将运行时的报错也输出到GUI页面中
    lh = logging.StreamHandler(stream=sys.stderr) if not handler else handler
    lh.setFormatter(lf)
    logger.addHandler(lh)
    logger.info("tyut_logger init done.")

    TYUT_LOGGER = logger
    TYUT_LOGGER_H = lh
    return TYUT_LOGGER


def get_logger():
    global TYUT_LOGGER
    if TYUT_LOGGER:
        return TYUT_LOGGER
    set_logger()
    return TYUT_LOGGER


NETWORK_AVAILABLE = None


def network_available():
    global NETWORK_AVAILABLE
    if NETWORK_AVAILABLE is not None:
        return NETWORK_AVAILABLE

    logger = get_logger()

    # List of reliable servers to check (IP, port)
    test_servers = [
        ("8.8.8.8", 53),          # Google DNS
        ("1.1.1.1", 53),          # Cloudflare DNS
        ("223.5.5.5", 53),        # Alibaba DNS (for China)
        ("114.114.114.114", 53),  # China Telecom DNS
    ]

    NETWORK_AVAILABLE = False
    for server_ip, port in test_servers:
        try:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.settimeout(1)
            s.connect((server_ip, port))
            s.close()
            logger.debug(f"Network available, connected to {server_ip}:{port}")
            NETWORK_AVAILABLE = True
            break
        except Exception as e:
            logger.debug(f"network check error: {str(e)}")
            continue

    return NETWORK_AVAILABLE


COUNTRY_CODE = ""  # "China" or other


def set_country_code():
    logger = get_logger()
    global COUNTRY_CODE
    if len(COUNTRY_CODE):
        return COUNTRY_CODE

    # Check network availability first
    if not network_available():
        logger.debug("Network not available, skipping country code detection")
        return COUNTRY_CODE

    logger.debug("getting country code...")
    try:
        response = requests.get('http://www.ip-api.com/json', timeout=2)
        response.raise_for_status()
        logger.debug(response.elapsed)

        result = response.json()
        country = result.get("country", "")
        logger.debug(f"country code: {country}")

        COUNTRY_CODE = country
    except requests.exceptions.RequestException as e:
        logger.warn(f"country code error: {e}")

    return COUNTRY_CODE


def get_country_code():
    global COUNTRY_CODE
    if len(COUNTRY_CODE):
        return COUNTRY_CODE
    return set_country_code()


def set_clis(clis):
    class CLIClass(click.MultiCommand):
        def list_commands(self, ctx):
            return list(clis.keys())

        def get_command(self, ctx, cmd_name):
            if cmd_name not in clis.keys():
                return None
            return clis[cmd_name]
    return CLIClass


class HexFormatter(object):
    '''
    from [esptool]
    '''

    def __init__(self, binary_string, auto_split=True):
        self._s = binary_string
        self._auto_split = auto_split

    def __str__(self):
        if self._auto_split and len(self._s) > 16:
            result = ""
            s = self._s
            while len(s) > 0:
                line = s[:16]
                ascii_line = "".join(
                    (
                        c
                        if (
                            c == " "
                            or (
                                    (c in string.printable)
                                    and (c not in string.whitespace))
                        )
                        else "."
                    )
                    for c in line.decode("ascii", "replace")
                )
                s = s[16:]
                result += "\n    %-16s %-16s | %s" % (
                    self.hexify(line[:8], False),
                    self.hexify(line[8:], False),
                    ascii_line,
                )
            return result
        else:
            return self.hexify(self._s, False)

    def hexify(self, s, uppercase=True):
        format_str = "%02X" if uppercase else "%02x"
        return "".join(format_str % c for c in s)
