#!/usr/bin/env python3
# coding=utf-8

import os
import time
import re
import struct
import itertools
import base64
from typing import Optional

from .reset import (
    DEFAULT_RESET_DELAY,
    ClassicReset,
    UnixTightReset,
    HardReset,
)

SYNC_TIMEOUT = 0.1
DEFAULT_TIMEOUT = 3
MAX_TIMEOUT = 240
MEM_END_ROM_TIMEOUT = 0.2
ERASE_WRITE_TIMEOUT_PER_MB = 40
WRITE_BLOCK_ATTEMPTS = 3
MD5_TIMEOUT_PER_MB = 8


def byte(bitstr, index):
    return bitstr[index]


def hexify(s, uppercase=True):
    format_str = "%02X" if uppercase else "%02x"
    return "".join(format_str % c for c in s)


def pad_to(data, alignment, pad_character=b"\xFF"):
    pad_mod = len(data) % alignment
    if pad_mod != 0:
        data += pad_character * (alignment - pad_mod)
    return data


def timeout_per_mb(seconds_per_mb, size_bytes):
    result = seconds_per_mb * (size_bytes / 1e6)
    if result < DEFAULT_TIMEOUT:
        return DEFAULT_TIMEOUT
    return result


def slip_reader(port, logger):
    def detect_panic_handler(input):
        guru_meditation = (
            rb"G?uru Meditation Error: (?:Core \d panic'ed \(([a-zA-Z ]*)\))?"
        )
        fatal_exception = rb"F?atal exception \(\d+\): (?:([a-zA-Z ]*)?.*epc)?"

        data = re.search(
            rb"".join([rb"(?:", guru_meditation, rb"|",
                       fatal_exception, rb")"]),
            input,
            re.DOTALL,
        )
        if data is not None:
            cause = [
                "({})".format(i.decode("utf-8"))
                for i in [data.group(1), data.group(2)]
                if i is not None
            ]
            cause = f" {cause[0]}" if len(cause) else ""
            msg = f"Guru Meditation Error detected{cause}"
            raise RuntimeError(msg)

    partial_packet = None
    in_escape = False
    successful_slip = False
    while True:
        waiting = port.inWaiting()
        read_bytes = port.read(1 if waiting == 0 else waiting)
        if read_bytes == b"":
            if partial_packet is None:  # fail due to no data
                msg = (
                    "Serial data stream stopped."
                    if successful_slip
                    else "No serial data received."
                )
            else:  # fail during packet transfer
                msg = "Packet content transfer stopped"
            raise RuntimeError(msg)
        logger.debug(f"Read {len(read_bytes)}: {read_bytes}")
        for b in read_bytes:
            b = bytes([b])
            if partial_packet is None:  # waiting for packet header
                if b == b"\xc0":
                    partial_packet = b""
                else:
                    logger.debug(f"Read invalid data: {read_bytes}")
                    remaining_data = port.read(port.inWaiting())
                    logger.debug(
                        f"Remaining data in serial buffer: {remaining_data}")
                    detect_panic_handler(read_bytes + remaining_data)
                    raise RuntimeError(
                        "Invalid head of packet."
                    )
            elif in_escape:  # part-way through escape sequence
                in_escape = False
                if b == b"\xdc":
                    partial_packet += b"\xc0"
                elif b == b"\xdd":
                    partial_packet += b"\xdb"
                else:
                    logger.debug(f"Read invalid data: {read_bytes}")
                    remaining_data = port.read(port.inWaiting())
                    logger.debug(
                        f"Remaining data in serial buffer: {remaining_data}")
                    detect_panic_handler(read_bytes + remaining_data)
                    raise RuntimeError(f"Invalid SLIP escape (0xdb, {b})")
            elif b == b"\xdb":  # start of escape sequence
                in_escape = True
            elif b == b"\xc0":  # end of packet
                logger.debug(f"Received full packet: {partial_packet}")
                yield partial_packet
                partial_packet = None
                successful_slip = True
            else:  # normal byte in packet
                partial_packet += b


class StubFlasher:
    def __init__(self, stub):
        self.text = base64.b64decode(stub["text"])
        self.text_start = stub["text_start"]
        self.entry = stub["entry"]

        try:
            self.data = base64.b64decode(stub["data"])
            self.data_start = stub["data_start"]
        except KeyError:
            self.data = None
            self.data_start = None

        self.bss_start = stub.get("bss_start")


class ESPLoader(object):
    CHIP_NAME = "Espressif device"
    IS_STUB = False
    STUB_CLASS: Optional[object] = None

    ESP_FLASH_BEGIN = 0x02
    # ESP_FLASH_DATA = 0x03
    # ESP_FLASH_END = 0x04
    ESP_MEM_BEGIN = 0x05
    ESP_MEM_END = 0x06
    ESP_MEM_DATA = 0x07
    ESP_SYNC = 0x08
    ESP_WRITE_REG = 0x09
    ESP_READ_REG = 0x0A

    ESP_SPI_SET_PARAMS = 0x0B
    # ESP_SPI_ATTACH = 0x0D
    # ESP_READ_FLASH_SLOW = 0x0E
    ESP_CHANGE_BAUDRATE = 0x0F
    ESP_FLASH_DEFL_BEGIN = 0x10
    ESP_FLASH_DEFL_DATA = 0x11
    ESP_FLASH_DEFL_END = 0x12
    ESP_SPI_FLASH_MD5 = 0x13

    ROM_INVALID_RECV_MSG = 0x05
    ESP_RAM_BLOCK = 0x1800
    STATUS_BYTES_LENGTH = 2
    ESP_CHECKSUM_MAGIC = 0xEF
    FLASH_WRITE_SIZE = 0x400

    CHIP_DETECT_MAGIC_REG_ADDR = 0x40001000
    UART_DATE_REG_ADDR = 0x60000078

    def __init__(self, port, logger):
        self._port = port
        self.logger = logger
        self.stub = None
        pass

    def read(self):
        return next(self._slip_reader)

    def write(self, packet):
        buf = (
            b"\xc0"
            + (packet.replace(b"\xdb",
                              b"\xdb\xdd").replace(b"\xc0",
                                                   b"\xdb\xdc"))
            + b"\xc0"
        )
        self._port.write(buf)

    @staticmethod
    def checksum(data, state=ESP_CHECKSUM_MAGIC):
        for b in data:
            state ^= b
        return state

    def command(
        self,
        op=None,
        data=b"",
        chk=0,
        wait_response=True,
        timeout=DEFAULT_TIMEOUT,
    ):
        """Send a request and read the response"""
        saved_timeout = self._port.timeout
        new_timeout = min(timeout, MAX_TIMEOUT)
        if new_timeout != saved_timeout:
            self._port.timeout = new_timeout

        try:
            if op is not None:
                pkt = struct.pack(b"<BBHI", 0x00, op, len(data), chk) + data
                self.write(pkt)

            if not wait_response:
                return

            for retry in range(100):
                p = self.read()
                if len(p) < 8:
                    continue
                (resp, op_ret, len_ret, val) = struct.unpack("<BBHI", p[:8])
                if resp != 1:
                    continue
                data = p[8:]

                if op is None or op_ret == op:
                    return val, data
                if byte(data, 0) != 0 \
                        and byte(data, 1) == self.ROM_INVALID_RECV_MSG:
                    self.flush_input()
                    raise

        finally:
            if new_timeout != saved_timeout:
                self._port.timeout = saved_timeout

        raise RuntimeError("Response doesn't match request")

    def flush_input(self):
        self._port.flushInput()
        self._slip_reader = slip_reader(self._port, self.logger)

    def sync(self):
        val, _ = self.command(
            self.ESP_SYNC,
            b"\x07\x07\x12\x20" + 32 * b"\x55",
            timeout=SYNC_TIMEOUT
        )
        self.sync_stub_detected = val == 0
        for _ in range(7):
            val, _ = self.command()
            self.sync_stub_detected &= val == 0

    def _connect_attempt(self, reset_strategy):
        self._port.reset_input_buffer()
        reset_strategy()  # Reset the chip to bootloader (download mode)
        waiting = self._port.inWaiting()
        read_bytes = self._port.read(waiting)
        self.logger.debug(f"connect read: {read_bytes}")

        for _ in range(5):
            try:
                self.flush_input()
                self._port.flushOutput()
                self.sync()
                return True
            except Exception as e:
                self.logger.error(f"Exception: {e}")
                time.sleep(0.05)
        return False

    def _construct_reset_strategy_sequence(self):
        delay = DEFAULT_RESET_DELAY
        extra_delay = DEFAULT_RESET_DELAY + 0.5
        if os.name != "nt" and not self._port.name.startswith("rfc2217:"):
            return (
                UnixTightReset(self._port, delay),
                UnixTightReset(self._port, extra_delay),
                ClassicReset(self._port, delay),
                ClassicReset(self._port, extra_delay),
            )

        return (
            ClassicReset(self._port, delay),
            ClassicReset(self._port, extra_delay),
        )

    def connect(self, is_stop, attempts=7):
        success = False
        reset_sequence = self._construct_reset_strategy_sequence()
        for _, reset_strategy in zip(
            range(attempts) if attempts > 0 else itertools.count(),
            itertools.cycle(reset_sequence),
        ):
            if is_stop():
                return False
            self.logger.debug(f"reset_strategy: {reset_strategy}")
            success = self._connect_attempt(reset_strategy)
            if success:
                break
        return success

    def read_reg(self, addr, timeout=DEFAULT_TIMEOUT):
        val, data = self.command(
            self.ESP_READ_REG, struct.pack("<I", addr), timeout=timeout
        )
        if byte(data, 0) != 0:
            self.logger.error(f"Failed to read register address {addr:#08x}")
            return None
        return val

    def write_reg(self, addr, value,
                  mask=0xFFFFFFFF, delay_us=0, delay_after_us=0):
        command = struct.pack("<IIII", addr, value, mask, delay_us)
        if delay_after_us > 0:
            command += struct.pack(
                "<IIII", self.UART_DATE_REG_ADDR, 0, 0, delay_after_us
            )

        return self.check_command("write target memory",
                                  self.ESP_WRITE_REG,
                                  command)

    def check_command(
        self, op_description, op=None, data=b"", chk=0, timeout=DEFAULT_TIMEOUT
    ):
        val, data = self.command(op, data, chk, timeout=timeout)

        if len(data) < self.STATUS_BYTES_LENGTH:
            self.logger.error(
                "Failed to %s. Only got %d byte status response."
                % (op_description, len(data))
            )
            return None
        status_bytes = data[-self.STATUS_BYTES_LENGTH:]
        if byte(status_bytes, 0) != 0:
            self.logger.error(
                f"Check command [{op_description}] failed: {status_bytes}"
            )
            return None

        if len(data) > self.STATUS_BYTES_LENGTH:
            return data[:-self.STATUS_BYTES_LENGTH]
        else:
            return val

    def mem_begin(self, size, blocks, blocksize, offset):
        if self.IS_STUB:
            stub = self.stub
            load_start = offset
            load_end = offset + size
            for stub_start, stub_end in [
                (stub.bss_start, stub.data_start + len(stub.data)),  # DRAM
                (stub.text_start, stub.text_start + len(stub.text)),  # IRAM
            ]:
                if load_start < stub_end and load_end > stub_start:
                    self.logger.error(
                        f"Software loader is resident at \
{stub_start:#08x} - {stub_end:#08x}. "
                        f"Can't load binary at overlapping address \
{load_start:#08x} - {load_end:#08x}. "
                    )
                    return None

        return self.check_command(
            "enter RAM download mode",
            self.ESP_MEM_BEGIN,
            struct.pack("<IIII", size, blocks, blocksize, offset),
        )

    def mem_block(self, data, seq):
        return self.check_command(
            "write to target RAM",
            self.ESP_MEM_DATA,
            struct.pack("<IIII", len(data), seq, 0, 0) + data,
            self.checksum(data),
        )

    def mem_finish(self, entrypoint=0):
        timeout = DEFAULT_TIMEOUT if self.IS_STUB else MEM_END_ROM_TIMEOUT
        data = struct.pack("<II", int(entrypoint == 0), entrypoint)
        try:
            return self.check_command(
                "leave RAM download mode",
                self.ESP_MEM_END,
                data=data,
                timeout=timeout
            )
        except RuntimeError:
            if self.IS_STUB:
                return None
            pass

    def run_stub(self, stub_flasher):
        if self.stub is None:
            self.stub = StubFlasher(stub_flasher)
            stub = self.stub

        if self.sync_stub_detected:
            self.logger.info("Stub is already running. Skip upload.")
            return self.STUB_CLASS(self)

        self.logger.info("Uploading stub...")
        for field in [stub.text, stub.data]:
            if field is not None:
                offs = stub.text_start if field == stub.text \
                    else stub.data_start
                length = len(field)
                blocks = (length+self.ESP_RAM_BLOCK-1) // self.ESP_RAM_BLOCK
                if self.mem_begin(length, blocks,
                                  self.ESP_RAM_BLOCK, offs) is None:
                    return None
                for seq in range(blocks):
                    from_offs = seq * self.ESP_RAM_BLOCK
                    to_offs = from_offs + self.ESP_RAM_BLOCK
                    if self.mem_block(field[from_offs:to_offs], seq) is None:
                        return None
        self.logger.debug("Running stub...")
        if self.mem_finish(stub.entry) is None:
            return None
        try:
            p = self.read()
        except StopIteration:
            self.logger.error("Failed to start stub. There was no response.")
            return None
        if p != b"OHAI":
            self.logger.error(f"Failed to start stub: {p}")
            return None
        return self.STUB_CLASS(self)

    def _set_port_baudrate(self, baud):
        try:
            self._port.baudrate = baud
        except IOError:
            return False
        return True

    def change_baud(self, baud):
        second_arg = self._port.baudrate if self.IS_STUB else 0
        self.command(self.ESP_CHANGE_BAUDRATE,
                     struct.pack("<II", baud, second_arg))
        if not self._set_port_baudrate(baud):
            return False
        time.sleep(0.05)
        self.flush_input()
        return True

    def flash_size_bytes(self, size):
        if "MB" in size:
            return int(size[: size.index("MB")]) * 1024 * 1024
        elif "KB" in size:
            return int(size[: size.index("KB")]) * 1024
        else:
            return None

    def flash_set_parameters(self, size):
        fl_id = 0
        total_size = size
        block_size = 64 * 1024
        sector_size = 4 * 1024
        page_size = 256
        status_mask = 0xFFFF
        self.check_command(
            "set SPI params",
            self.ESP_SPI_SET_PARAMS,
            struct.pack(
                "<IIIIII",
                fl_id,
                total_size,
                block_size,
                sector_size,
                page_size,
                status_mask,
            ),
        )

    def run_spiflash_command(
        self,
        spiflash_command,
        data=b"",
        read_bits=0,
        addr=None,
        addr_len=0,
        dummy_len=0,
    ):
        SPI_USR_COMMAND = 1 << 31
        SPI_USR_ADDR = 1 << 30
        SPI_USR_DUMMY = 1 << 29
        SPI_USR_MISO = 1 << 28
        SPI_USR_MOSI = 1 << 27

        base = self.SPI_REG_BASE
        SPI_CMD_REG = base + 0x00
        SPI_ADDR_REG = base + 0x04
        SPI_USR_REG = base + self.SPI_USR_OFFS
        SPI_USR1_REG = base + self.SPI_USR1_OFFS
        SPI_USR2_REG = base + self.SPI_USR2_OFFS
        SPI_W0_REG = base + self.SPI_W0_OFFS

        def set_data_lengths(mosi_bits, miso_bits):
            SPI_MOSI_DLEN_REG = base + self.SPI_MOSI_DLEN_OFFS
            SPI_MISO_DLEN_REG = base + self.SPI_MISO_DLEN_OFFS
            if mosi_bits > 0:
                self.write_reg(SPI_MOSI_DLEN_REG, mosi_bits - 1)
            if miso_bits > 0:
                self.write_reg(SPI_MISO_DLEN_REG, miso_bits - 1)
            flags = 0
            if dummy_len > 0:
                flags |= dummy_len - 1
            if addr_len > 0:
                flags |= (addr_len - 1) << SPI_USR_ADDR_LEN_SHIFT
            if flags:
                self.write_reg(SPI_USR1_REG, flags)

        SPI_CMD_USR = 1 << 18

        SPI_USR2_COMMAND_LEN_SHIFT = 28
        SPI_USR_ADDR_LEN_SHIFT = 26

        if read_bits > 32:
            self.logger.error(
                "Reading more than 32 bits back from a SPI flash "
                "operation is unsupported"
            )
            return None
        if len(data) > 64:
            self.logger.error(
                "Writing more than 64 bytes of data with one SPI "
                "command is unsupported"
            )
            return None

        data_bits = len(data) * 8
        old_spi_usr = self.read_reg(SPI_USR_REG)
        old_spi_usr2 = self.read_reg(SPI_USR2_REG)
        flags = SPI_USR_COMMAND
        if read_bits > 0:
            flags |= SPI_USR_MISO
        if data_bits > 0:
            flags |= SPI_USR_MOSI
        if addr_len > 0:
            flags |= SPI_USR_ADDR
        if dummy_len > 0:
            flags |= SPI_USR_DUMMY
        set_data_lengths(data_bits, read_bits)
        self.write_reg(SPI_USR_REG, flags)
        self.write_reg(
            SPI_USR2_REG, (7 << SPI_USR2_COMMAND_LEN_SHIFT) | spiflash_command
        )
        if addr and addr_len > 0:
            self.write_reg(SPI_ADDR_REG, addr)
        if data_bits == 0:
            self.write_reg(SPI_W0_REG, 0)
        else:
            data = pad_to(data, 4, b"\00")  # pad to 32-bit multiple
            words = struct.unpack("I" * (len(data) // 4), data)
            next_reg = SPI_W0_REG
            for word in words:
                self.write_reg(next_reg, word)
                next_reg += 4
        self.write_reg(SPI_CMD_REG, SPI_CMD_USR)

        def wait_done():
            for _ in range(10):
                if (self.read_reg(SPI_CMD_REG) & SPI_CMD_USR) == 0:
                    return True
            self.logger.error("SPI command did not complete in time")
            return False

        if not wait_done():
            return None

        status = self.read_reg(SPI_W0_REG)
        self.write_reg(SPI_USR_REG, old_spi_usr)
        self.write_reg(SPI_USR2_REG, old_spi_usr2)
        return status

    def flash_defl_begin(self, size, compsize, offset):
        self.logger.debug("func: flash_defl_begin()")
        num_blocks = (compsize + self.FLASH_WRITE_SIZE - 1) \
            // self.FLASH_WRITE_SIZE

        write_size = size
        timeout = DEFAULT_TIMEOUT
        self.logger.debug(f"Compressed {size} bytes to {compsize}")
        params = struct.pack(
            "<IIII", write_size, num_blocks, self.FLASH_WRITE_SIZE, offset
        )
        self.check_command(
            "enter compressed flash mode",
            self.ESP_FLASH_DEFL_BEGIN,
            params,
            timeout=timeout,
        )
        return num_blocks

    def flash_defl_block(self, data, seq, timeout=DEFAULT_TIMEOUT):
        self.logger.debug("func: flash_defl_block()")
        for attempts_left in range(WRITE_BLOCK_ATTEMPTS - 1, -1, -1):
            try:
                self.check_command(
                    "write compressed data to flash after seq %d" % seq,
                    self.ESP_FLASH_DEFL_DATA,
                    struct.pack("<IIII", len(data), seq, 0, 0) + data,
                    self.checksum(data),
                    timeout=timeout,
                )
                break
            except RuntimeError:
                if attempts_left:
                    self.logger.warning(
                        "Compressed block write failed, "
                        f"retrying with {attempts_left} attempts left"
                    )
                else:
                    return False
        return True

    def flash_defl_finish(self, reboot=False):
        self.logger.debug("func: flash_defl_finish()")
        if not reboot and not self.IS_STUB:
            return
        pkt = struct.pack("<I", int(not reboot))
        self.check_command("leave compressed flash mode",
                           self.ESP_FLASH_DEFL_END,
                           pkt)

    def flash_md5sum(self, addr, size):
        timeout = timeout_per_mb(MD5_TIMEOUT_PER_MB, size)
        res = self.check_command(
            "calculate md5sum",
            self.ESP_SPI_FLASH_MD5,
            struct.pack("<IIII", addr, size, 0, 0),
            timeout=timeout,
        )

        if len(res) == 32:
            return res.decode("utf-8")  # already hex formatted
        elif len(res) == 16:
            return hexify(res).lower()
        else:
            self.logger.error(
                "MD5Sum command returned unexpected result: %r" % res
            )
            return None

    def flash_begin(self, size, offset, begin_rom_encrypted=False):
        self.logger.debug("func: flash_begin()")
        num_blocks = (size + self.FLASH_WRITE_SIZE - 1) \
            // self.FLASH_WRITE_SIZE
        erase_size = self.get_erase_size(offset, size)
        timeout = DEFAULT_TIMEOUT

        params = struct.pack(
            "<IIII", erase_size, num_blocks, self.FLASH_WRITE_SIZE, offset
        )
        self.check_command(
            "enter Flash download mode",
            self.ESP_FLASH_BEGIN,
            params,
            timeout=timeout
        )
        return num_blocks

    def flash_id(self):
        SPIFLASH_RDID = 0x9F
        flash_id = self.run_spiflash_command(SPIFLASH_RDID, b"", 24)
        return flash_id

    def hard_reset(self):
        HardReset(self._port)()
