#!/usr/bin/env python
# -*- coding: utf-8 -*-

from .flash.choose_port import choose_port

__all__ = [
    "choose_port",
    "cli",
]


def cli():
    from .main import run
    run()
