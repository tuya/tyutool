#!/usr/bin/env python
# -*- coding: utf-8 -*-


def _lazy_web_monitor():
    from .web_debug import WebAIDebugMonitor
    return WebAIDebugMonitor


def _lazy_ser_monitor():
    from .ser_debug import SerAIDebugMonitor
    return SerAIDebugMonitor


def _lazy_type_mapping():
    from .web.protocol import TYPE_MAPPING
    return TYPE_MAPPING


def __getattr__(name):
    if name == "WebAIDebugMonitor":
        return _lazy_web_monitor()
    elif name == "SerAIDebugMonitor":
        return _lazy_ser_monitor()
    elif name == "TYPE_MAPPING":
        return _lazy_type_mapping()
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")


__all__ = [
    "TYPE_MAPPING",
    "WebAIDebugMonitor",
    "SerAIDebugMonitor",
]
