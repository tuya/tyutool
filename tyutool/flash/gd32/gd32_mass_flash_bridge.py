#!/usr/bin/env python
# -*- coding: utf-8 -*-

import importlib.util
from pathlib import Path


GD32_DEVICE = "GD32VW553H"
GD32_SCRIPT_RELATIVE_PATH = Path("platform") / "GD32" / "tools" / "gd32_mass_flash.py"


def _find_gd32_mass_flash_script():
    for parent in Path(__file__).resolve().parents:
        candidate = parent / GD32_SCRIPT_RELATIVE_PATH
        if candidate.is_file():
            return candidate
    return None


def try_flash_gd32_device(device, port, baud, start, binfile, logger):
    if str(device).upper() != GD32_DEVICE:
        return False, False

    script_path = _find_gd32_mass_flash_script()
    if script_path is None:
        logger.error("Cannot find platform/GD32/tools/gd32_mass_flash.py")
        return True, False

    try:
        spec = importlib.util.spec_from_file_location("gd32_mass_flash", script_path)
        if spec is None or spec.loader is None:
            logger.error(f"Cannot load script: {script_path}")
            return True, False

        gd32_mass_flash = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(gd32_mass_flash)
        start_arg = f"{start:#x}" if start is not None else None
        result = gd32_mass_flash.flash_firmware(
            device=device,
            port=port,
            baud=baud,
            start=start_arg,
            binfile=binfile,
            logger=logger,
        )
        if result.get("success"):
            logger.info("GD32 mass flash success.")
        else:
            logger.error(f'GD32 mass flash failed: {result.get("message", "")}')
        return True, bool(result.get("success"))
    except Exception as exc:
        logger.exception(f"GD32 mass flash exception: {exc}")
        return True, False
