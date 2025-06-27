#!/usr/bin/env python3
# -*- coding=utf-8 -*-

import sys
import serial
import time


def serial_write_idf(data, sleep,
                     port, baud):
    print(f"{port}({baud}) write_idf ...")
    ser = serial.serial_for_url(port, exclusive=True, do_not_open=True)
    ser.open()
    ser.baudrate = baud
    ser.write_timeout = 10
    ser.write(data)
    # time.sleep(sleep)
    pass


def serial_write(data, size, sleep,
                 port, baud, timeout=1):
    print(f"{port}({baud}) write ...")
    ser = serial.Serial(port, baud, timeout=timeout)
    if ser.isOpen():
        total = len(data)
        count = int(total / size)
        rem = total % size
        for i in range(count):
            # print(f"i: {i}")
            ser.write(data[i*size:(i+1)*size])
            time.sleep(sleep)
        if rem:
            # print(f"rem: {rem}")
            ser.write(data[-rem:])
    else:
        ser.close()
        print(f"{port} open failed.")
    pass


def serial_read(port, baud, timeout=1):
    ser = serial.Serial(port, baud, timeout=timeout)
    if ser.isOpen():
        print(f"{port}({baud}) read ...")
        total = 0

        while True:
            line = ser.readline()
            total += len(line)
            if line:
                print(f"buf({total}): {line}")
    else:
        ser.close()
        print(f"{port} open failed.")
    pass


if __name__ == "__main__":
    port = "/dev/tty.usbserial-0001"
    baud = 921600
    data = bytes(list(range(256)) * 4 * 4)
    size = 1024
    sleep = 0.1

    if len(sys.argv) > 1 and sys.argv[1] == "write":
        serial_write(data, size, sleep,
                     port, baud)
    elif len(sys.argv) > 1 and sys.argv[1] == "read":
        serial_read(port, baud)
        pass
    elif len(sys.argv) > 1 and sys.argv[1] == "write_idf":
        serial_write_idf(data, sleep, port, baud)
        pass
    else:
        print("need cmd [write/read]")
