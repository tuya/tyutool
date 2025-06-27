# -*- coding=utf-8 -*-
import sys
import serial
import re
import time
import struct
import fcntl
import termios
import json
import base64
import zlib
import hashlib
import itertools

# Constants used for terminal status lines reading/setting.
# Taken from pySerial's backend for IO:
# https://github.com/pyserial/pyserial/blob/master/serial/serialposix.py
TIOCMSET = getattr(termios, "TIOCMSET", 0x5418)
TIOCMGET = getattr(termios, "TIOCMGET", 0x5415)
TIOCM_DTR = getattr(termios, "TIOCM_DTR", 0x002)
TIOCM_RTS = getattr(termios, "TIOCM_RTS", 0x004)


def byte(bitstr, index):
    return bitstr[index]


def hexify(s, uppercase=True):
    format_str = "%02X" if uppercase else "%02x"
    return "".join(format_str % c for c in s)


def timeout_per_mb(seconds_per_mb, size_bytes):
    """Scales timeouts which are size-specific"""
    result = seconds_per_mb * (size_bytes / 1e6)
    if result < 3:
        return 3
    return result


def flash_size_bytes(size):
    """Given a flash size of the type passed in args.flash_size
    (ie 512KB or 1MB) then return the size in bytes.
    """
    if size is None:
        return None
    if "MB" in size:
        return int(size[: size.index("MB")]) * 1024 * 1024
    elif "KB" in size:
        return int(size[: size.index("KB")]) * 1024
    else:
        raise Exception


def pad_to(data, alignment, pad_character=b"\xFF"):
    """Pad to the next alignment boundary"""
    pad_mod = len(data) % alignment
    if pad_mod != 0:
        data += pad_character * (alignment - pad_mod)
    return data


def _update_image_flash_params(esp, address, args, image):
    magic, _, flash_mode, flash_size_freq = struct.unpack("BBBB", image[:4])
    if address != 0x0:
        return image  # not flashing bootloader offset, so don't modify this

    sha_appended = args.chip != "esp8266" and image[8 + 15] == 1

    if args.flash_mode != "keep":
        flash_mode = 2

    flash_freq = flash_size_freq & 0x0F
    if args.flash_freq != "keep":
        flash_freq = 0x0F

    flash_size = flash_size_freq & 0xF0
    if args.flash_size != "keep":
        flash_size = 0x10

    flash_params = struct.pack(b"BB", flash_mode, flash_size + flash_freq)
    if flash_params != image[2:4]:
        print("Flash params set to 0x%04x" % struct.unpack(">H", flash_params))
        image = image[0:2] + flash_params + image[4:]

    if sha_appended:
        # image_object = esp.BOOTLOADER_IMAGE(io.BytesIO(image))
        image_data_before_sha = image[: 21040]
        image_data_after_sha = image[
            (21040 + 32):
        ]

        sha_digest_calculated = hashlib.sha256(image_data_before_sha).digest()
        image = bytes(
            itertools.chain(
                image_data_before_sha,
                sha_digest_calculated,
                image_data_after_sha
            )
        )

        image_stored_sha = image[
            21040:21040 + 32
        ]

        if hexify(sha_digest_calculated) == hexify(image_stored_sha):
            print("SHA digest in image updated")
        else:
            print(
                "WARNING: SHA recalculation for binary failed!\n"
            )

    return image


def write_flash(esp, args):
    # args.compress = True
    all_files = [
        (offs, filename, False) for (offs, filename) in args.addr_filename
    ]
    for address, argfile, encrypted in all_files:
        image = pad_to(argfile.read(), 4)
        image = _update_image_flash_params(esp, address, args, image)

        calcmd5 = hashlib.md5(image).hexdigest()
        uncsize = len(image)
        uncimage = image
        image = zlib.compress(uncimage, 9)
        # Decompress the compressed binary a block at a time,
        # to dynamically calculate the timeout based on the real write size
        decompress = zlib.decompressobj()
        blocks = esp.flash_defl_begin(uncsize, len(image), address)

        argfile.seek(0)  # in case we need it again
        seq = 0
        bytes_sent = 0  # bytes sent on wire
        bytes_written = 0  # bytes written to flash
        t = time.time()

        timeout = 3
        while len(image) > 0:
            print(
                "Writing at 0x%08x... (%d %%)"
                % (address + bytes_written, 100 * (seq + 1) // blocks)
            )
            sys.stdout.flush()
            block = image[0:esp.FLASH_WRITE_SIZE]

            block_uncompressed = len(decompress.decompress(block))
            bytes_written += block_uncompressed
            block_timeout = max(
                3,
                timeout_per_mb(40, block_uncompressed),
            )
            esp.flash_defl_block(block, seq, timeout=timeout)
            timeout = block_timeout

            bytes_sent += len(block)
            image = image[esp.FLASH_WRITE_SIZE:]
            seq += 1

        esp.read_reg(0x40001000, timeout=timeout)
        t = time.time() - t
        speed_msg = ""
        if t > 0.0:
            speed_msg = " (effective %.1f kbit/s)" % (uncsize / t * 8 / 1000)
        print(
            "Wrote %d bytes (%d compressed) at 0x%08x in %.1f seconds%s..."
            % (uncsize, bytes_sent, address, t, speed_msg)
        )
        res = esp.flash_md5sum(address, uncsize)
        if res != calcmd5:
            print("File  md5: %s" % calcmd5)
            print("Flash md5: %s" % res)
            print(
                "MD5 of 0xFF is %s"
                % (hashlib.md5(b"\xFF" * uncsize).hexdigest())
            )
            raise
        else:
            print("Hash of data verified.")

    print("\nLeaving...")

    esp.flash_begin(0, 0)
    esp.flash_defl_finish(False)
    pass


class UnixTightReset(object):
    def __init__(self, port, reset_delay=0.05):
        self.port = port
        self.reset_delay = reset_delay

    def __call__(self):
        self._setDTRandRTS(False, False)
        self._setDTRandRTS(True, True)
        self._setDTRandRTS(False, True)  # IO0=HIGH & EN=LOW, chip in reset
        time.sleep(0.1)
        self._setDTRandRTS(True, False)  # IO0=LOW & EN=HIGH, chip out of reset
        time.sleep(self.reset_delay)
        self._setDTRandRTS(False, False)  # IO0=HIGH, done
        self._setDTR(False)  # Needed in some environments to ensure IO0=HIGH

    def _setDTR(self, state):
        self.port.setDTR(state)

    def _setRTS(self, state):
        self.port.setRTS(state)
        self.port.setDTR(self.port.dtr)

    def _setDTRandRTS(self, dtr=False, rts=False):
        status = struct.unpack(
            "I", fcntl.ioctl(self.port.fileno(), TIOCMGET, struct.pack("I", 0))
        )[0]
        if dtr:
            status |= TIOCM_DTR
        else:
            status &= ~TIOCM_DTR
        if rts:
            status |= TIOCM_RTS
        else:
            status &= ~TIOCM_RTS
        fcntl.ioctl(self.port.fileno(), TIOCMSET, struct.pack("I", status))


class HardReset(UnixTightReset):
    def __init__(self, port, uses_usb_otg=False):
        super().__init__(port)
        self.uses_usb_otg = uses_usb_otg

    def reset(self):
        self._setRTS(True)  # EN->LOW
        if self.uses_usb_otg:
            time.sleep(0.2)
            self._setRTS(False)
            time.sleep(0.2)
        else:
            time.sleep(0.1)
            self._setRTS(False)


def slip_reader(port):
    def detect_panic_handler(input):
        guru_meditation = (
            rb"G?uru Meditation Error: (?:Core \d panic'ed \(([a-zA-Z ]*)\))?"
        )
        fatal_exception = rb"F?atal exception \(\d+\): (?:([a-zA-Z ]*)?.*epc)?"

        # Search either for Guru Meditation or Fatal Exception
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
            raise ValueError(msg)

    partial_packet = None
    in_escape = False
    successful_slip = False
    while True:
        waiting = port.inWaiting()
        read_bytes = port.read(1 if waiting == 0 else waiting)
        if read_bytes == b"":
            if partial_packet is None:  # fail due to no data
                msg = (
                    "Possible serial noise or corruption."
                    if successful_slip
                    else "No serial data received."
                )
            else:  # fail during packet transfer
                msg = "Transfer stopped (received {} bytes)".format(
                    len(partial_packet)
                )
            raise ValueError(msg)
        for b in read_bytes:
            b = bytes([b])
            if partial_packet is None:  # waiting for packet header
                if b == b"\xc0":
                    partial_packet = b""
                else:
                    remaining_data = port.read(port.inWaiting())
                    detect_panic_handler(read_bytes + remaining_data)
                    raise
            elif in_escape:  # part-way through escape sequence
                in_escape = False
                if b == b"\xdc":
                    partial_packet += b"\xc0"
                elif b == b"\xdd":
                    partial_packet += b"\xdb"
                else:
                    remaining_data = port.read(port.inWaiting())
                    detect_panic_handler(read_bytes + remaining_data)
                    raise
            elif b == b"\xdb":  # start of escape sequence
                in_escape = True
            elif b == b"\xc0":  # end of packet
                yield partial_packet
                partial_packet = None
                successful_slip = True
            else:  # normal byte in packet
                partial_packet += b


class StubFlasher:
    def __init__(self, json_path):
        with open(json_path) as json_file:
            stub = json.load(json_file)

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
    ESP_SYNC = 0x08
    ESP_RAM_BLOCK = 0x1800
    IS_STUB = False
    STATUS_BYTES_LENGTH = 2
    ESP_FLASH_BEGIN = 0x02
    ESP_MEM_BEGIN = 0x05
    ESP_MEM_END = 0x06
    ESP_MEM_DATA = 0x07
    ESP_CHANGE_BAUDRATE = 0x0F
    ESP_SPI_SET_PARAMS = 0x0B
    ESP_SPI_FLASH_MD5 = 0x13
    FLASH_WRITE_SIZE = 0x400

    def __init__(self, port, baud):
        self.secure_download_mode = False
        self.stub_is_disabled = False
        self.cache = {
            "flash_id": None,
            "chip_id": None,
            "uart_no": None,
            "usb_pid": None,
        }

        # Device-and-runtime-specific cache
        self.cache = {
            "flash_id": None,
            "chip_id": None,
            "uart_no": None,
            "usb_pid": None,
        }

        self._port = serial.serial_for_url(
            port, exclusive=True, do_not_open=True
        )
        if sys.platform == "win32":
            # When opening a port on Windows,
            # the RTS/DTR (active low) lines
            # need to be set to False (pulled high)
            # to avoid unwanted chip reset
            self._port.rts = False
            self._port.dtr = False
        self._port.open()

        self._slip_reader = slip_reader(self._port)
        self._port.baudrate = baud
        self._port.write_timeout = 10
        pass

    def flush_input(self):
        self._port.flushInput()
        self._slip_reader = slip_reader(self._port)

    def sync(self):
        val, _ = self.command(
            self.ESP_SYNC, b"\x07\x07\x12\x20" + 32 * b"\x55", timeout=0.1
        )
        self.sync_stub_detected = val == 0

        for _ in range(7):
            val, _ = self.command()
            self.sync_stub_detected &= val == 0

    def _connect_attempt(self, reset_strategy):
        """A single connection attempt"""
        last_error = None
        boot_log_detected = False

        self._port.reset_input_buffer()
        reset_strategy()  # Reset the chip to bootloader (download mode)
        waiting = self._port.inWaiting()
        read_bytes = self._port.read(waiting)
        data = re.search(
            b"boot:(0x[0-9a-fA-F]+)(.*waiting for download)?",
            read_bytes, re.DOTALL
        )
        if data is not None:
            boot_log_detected = True

        for _ in range(5):
            try:
                self.flush_input()
                self._port.flushOutput()
                self.sync()
                return None
            except Exception as e:
                print(".", end="")
                sys.stdout.flush()
                time.sleep(0.05)
                last_error = e

        if boot_log_detected:
            last_error = Exception
        return last_error

    def write(self, packet):
        """Write bytes to the serial port while performing SLIP escaping"""
        buf = (
            b"\xc0"
            + (packet.replace(b"\xdb",
                              b"\xdb\xdd").replace(b"\xc0",
                                                   b"\xdb\xdc"))
            + b"\xc0"
        )
        self._port.write(buf)

    def read(self):
        return next(self._slip_reader)

    def read_reg(self, addr, timeout=3):
        val, data = self.command(
            0x0A, struct.pack("<I", addr), timeout=timeout
        )
        if byte(data, 0) != 0:
            raise
        return val

    @staticmethod
    def checksum(data, state=0xEF):
        for b in data:
            state ^= b
        return state

    def command(
        self,
        op=None,
        data=b"",
        chk=0,
        wait_response=True,
        timeout=3,
    ):
        saved_timeout = self._port.timeout
        new_timeout = min(timeout, 240)
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
                if byte(data, 0) != 0 and byte(data, 1) == 0x05:
                    self.flush_input()
                    raise
        finally:
            if new_timeout != saved_timeout:
                self._port.timeout = saved_timeout
        raise

    def check_command(
        self, op_description, op=None, data=b"", chk=0, timeout=3
    ):
        val, data = self.command(op, data, chk, timeout=timeout)

        if len(data) < self.STATUS_BYTES_LENGTH:
            raise Exception
        status_bytes = data[-self.STATUS_BYTES_LENGTH:]
        if byte(status_bytes, 0) != 0:
            raise Exception

        if len(data) > self.STATUS_BYTES_LENGTH:
            return data[: -self.STATUS_BYTES_LENGTH]
        else:
            return val

    def connect(self):
        print("Connecting...", end="")
        sys.stdout.flush()
        reset_strategy = UnixTightReset(self._port)

        self._port.reset_input_buffer()
        reset_strategy()  # Reset the chip to bootloader (download mode)

        try:
            self._connect_attempt(reset_strategy)
        finally:
            print("")  # end 'Connecting...' line

    def mem_begin(self, size, blocks, blocksize, offset):
        if self.IS_STUB:
            stub_json = "./esptool/targets/stub_flasher/stub_flasher_32s3.json"
            stub = StubFlasher(stub_json)
            load_start = offset
            load_end = offset + size
            for stub_start, stub_end in [
                (stub.bss_start, stub.data_start + len(stub.data)),
                (stub.text_start, stub.text_start + len(stub.text)),
            ]:
                if load_start < stub_end and load_end > stub_start:
                    raise Exception

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
        timeout = 3 if self.IS_STUB else 0.2
        data = struct.pack("<II", int(entrypoint == 0), entrypoint)
        try:
            return self.check_command(
                "leave RAM download mode",
                self.ESP_MEM_END, data=data, timeout=timeout
            )
        except Exception:
            if self.IS_STUB:
                raise
            pass

    def run_stub(self, stub=None):
        if stub is None:
            stub_json = "./esptool/targets/stub_flasher/stub_flasher_32s3.json"
            stub = StubFlasher(stub_json)

        if self.sync_stub_detected:
            print("Stub is already running. No upload is necessary.")
            return None

        # Upload
        print("Uploading stub...")
        for field in [stub.text, stub.data]:
            if field is not None:
                if field == stub.text:
                    offs = stub.text_start
                else:
                    offs = stub.data_start
                length = len(field)
                blocks = (length + self.ESP_RAM_BLOCK - 1) \
                    // self.ESP_RAM_BLOCK
                self.mem_begin(length, blocks, self.ESP_RAM_BLOCK, offs)
                for seq in range(blocks):
                    from_offs = seq * self.ESP_RAM_BLOCK
                    to_offs = from_offs + self.ESP_RAM_BLOCK
                    self.mem_block(field[from_offs:to_offs], seq)
        print("Running stub...")
        self.mem_finish(stub.entry)
        try:
            p = self.read()
        except StopIteration:
            raise Exception

        if p != b"OHAI":
            raise Exception
        print("Stub running...")
        return None

    def change_baud(self, baud):
        print("Changing baud rate to %d" % baud)
        # stub takes the new baud rate and the old one
        second_arg = self._port.baudrate if self.IS_STUB else 0
        self.command(self.ESP_CHANGE_BAUDRATE, struct.pack("<II", baud,
                                                           second_arg))
        print("Changed.")
        self._port.baudrate = baud
        time.sleep(0.05)  # get rid of crap sent during baud rate change
        self.flush_input()

    def flash_id(self):
        return 0x3980C2

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

    def flash_defl_begin(self, size, compsize, offset):
        num_blocks = (compsize + self.FLASH_WRITE_SIZE - 1) \
            // self.FLASH_WRITE_SIZE

        write_size = size
        timeout = 3
        print("Compressed %d bytes to %d..." % (size, compsize))
        params = struct.pack(
            "<IIII", write_size, num_blocks, self.FLASH_WRITE_SIZE, offset
        )
        self.check_command(
            "enter compressed flash mode",
            0x10,
            params,
            timeout=timeout,
        )
        return num_blocks

    def flash_defl_block(self, data, seq, timeout=3):
        for attempts_left in range(3 - 1, -1, -1):
            try:
                self.check_command(
                    "write compressed data to flash after seq %d" % seq,
                    0x11,
                    struct.pack("<IIII", len(data), seq, 0, 0) + data,
                    self.checksum(data),
                    timeout=timeout,
                )
                break
            except Exception:
                raise

    def flash_defl_finish(self, reboot=False):
        """Leave compressed flash mode and run/reboot"""
        if not reboot and not self.IS_STUB:
            return
        pkt = struct.pack("<I", int(not reboot))
        self.check_command("leave compressed flash mode",
                           self.ESP_FLASH_DEFL_END, pkt)
        self.in_bootloader = False

    def flash_begin(self, size, offset, begin_rom_encrypted=False):
        num_blocks = (size + self.FLASH_WRITE_SIZE - 1) \
            // self.FLASH_WRITE_SIZE
        erase_size = size

        t = time.time()
        if self.IS_STUB:
            timeout = 3
        else:
            timeout = timeout_per_mb(
                30, size
            )  # ROM performs the erase up front

        params = struct.pack(
            "<IIII", erase_size, num_blocks, self.FLASH_WRITE_SIZE, offset
        )
        self.check_command(
            "enter Flash download mode",
            self.ESP_FLASH_BEGIN, params, timeout=timeout
        )
        if size != 0 and not self.IS_STUB:
            print("Took %.2fs to erase flash block" % (time.time() - t))
        return num_blocks

    def hard_reset(self):
        uses_usb_otg = False

        try:
            self.write_reg(
                0x6000812C, 0, 0x1
            )
        except Exception:
            pass

        print("Hard resetting via RTS pin...")
        HardReset(self._port, uses_usb_otg)()
        pass

    def flash_md5sum(self, addr, size):
        timeout = timeout_per_mb(8, size)
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
            raise


class ARGS:
    def __init__(self, baud):
        self.baud = baud
        self.chip = "esp32s3"
        self.flash_mode = "dio"
        self.flash_freq = "80m"
        self.flash_size = "2MB"
        self.addr_filename = [
            (0x0, open("esp32s3_bin/bootloader.bin", "rb")),
            (0x8000, open("esp32s3_bin/partition-table.bin", "rb")),
            (0x10000, open("esp32s3_bin/hello_world.bin", "rb")),
        ]
        pass


def main():
    port = "/dev/cu.usbserial-1140"
    baud = 115200
    initial_baud = 115200

    args = ARGS(baud)
    esp = ESPLoader(port, initial_baud)
    esp.connect()
    esp.run_stub()
    if args.baud > initial_baud:
        esp.change_baud(args.baud)

    print("Configuring flash size...")
    esp.flash_set_parameters(flash_size_bytes(args.flash_size))
    write_flash(esp, args)
    esp.hard_reset()


if __name__ == "__main__":
    main()
