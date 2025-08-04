#!/usr/bin/env python3
# coding=utf-8

import sys
from serial.tools import list_ports


def choose_port():
    port_list = list(list_ports.comports())
    port_items = []
    for p in port_list:
        if p.device.startswith("/dev/ttyS"):
            continue
        port_items.append(p.device)
    if len(port_items) == 0:
        print("No serial port is available.")
        sys.exit(0)
    if len(port_items) == 1:
        return port_items[0]

    port_items.sort()
    print("--------------------")
    for i in range(len(port_items)):
        print(f"{i+1}. {port_items[i]}")
    print("--------------------")
    while True:
        try:
            num = int(input("Select serial port: "))
            if 1 <= num <= len(port_items):
                return port_items[num-1]
        except ValueError:
            continue
        except KeyboardInterrupt:
            sys.exit(0)
