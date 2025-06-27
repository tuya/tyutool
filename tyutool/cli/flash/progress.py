#!/usr/bin/env python
# -*- coding: utf-8 -*-


import os
from rich.progress import Progress, TextColumn, BarColumn, TimeElapsedColumn, \
                        TimeRemainingColumn, SpinnerColumn, TransferSpeedColumn
from tqdm import tqdm

from tyutool.flash import ProgressHandler


class CliProgressHandler(ProgressHandler):
    def __init__(self):
        super().__init__()
        self.pg = Progress(
            TextColumn("[progress.description]{task.description}:",
                       justify="right"),
            BarColumn(),
            TextColumn("[progress.percentage]{task.percentage:>3.0f}%"),
            TransferSpeedColumn(),
            SpinnerColumn(),
            TimeElapsedColumn(),
            "/", TimeRemainingColumn(),
            transient=True,
              )
        self.task = None
        pass

    def setup(self, header, total):
        color_header = header
        if "Erasing" in header:
            color_header = "[bold red]Erasing[/bold red]"
        elif "Writing" in header:
            color_header = "[bold green]Writing[/bold green]"
        elif "Reading" in header:
            color_header = "[bold green]Reading[/bold green]"
        elif "CRCChecking" in header:
            color_header = "[bold yellow]CRCChecking[/bold yellow]"
        self.task = self.pg.add_task(description=color_header, total=total)
        pass

    def start(self):
        self.pg.start()
        pass

    def update(self, size=1):
        self.pg.advance(self.task, advance=size)
        pass

    def close(self):
        if self.task is None:
            return
        self.pg.stop_task(self.task)
        self.pg.remove_task(self.task)
        os.system("echo '\033[?25h'")  # 解决进度条显示完后光标消失的问题
        self.task = None

    def __del__(self):
        # 解决中途退出后光标消失问题
        os.system("echo '\033[?25h'")


class CliProgressHandlerTqdm(ProgressHandler):
    '''
    使用tqdm的进度条是因为，
    在Arduino中使用烧录工具时，rich的显示异常。
    '''

    def __init__(self):
        self.pg = None
        pass

    def setup(self,
              header: str,
              total: int) -> None:
        self.pg = tqdm(total=total, desc=header, ascii=True)
        pass

    def start(self) -> None:
        pass

    def update(self, size: int = 1) -> None:
        self.pg.update(size)
        pass

    def close(self) -> None:
        self.pg.close()
        pass
