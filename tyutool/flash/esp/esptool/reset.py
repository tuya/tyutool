import os
import time
import struct

if os.name != "nt":
    import fcntl
    import termios

    TIOCMSET = getattr(termios, "TIOCMSET", 0x5418)
    TIOCMGET = getattr(termios, "TIOCMGET", 0x5415)
    TIOCM_DTR = getattr(termios, "TIOCM_DTR", 0x002)
    TIOCM_RTS = getattr(termios, "TIOCM_RTS", 0x004)


DEFAULT_RESET_DELAY = 0.05


class ResetStrategy(object):
    def __init__(self, port,
                 reset_delay=DEFAULT_RESET_DELAY):
        self.port = port
        self.reset_delay = reset_delay

    def __call__(self):
        try:
            self.reset()
        except Exception as e:
            print(f"Exception: {e}")
            raise

    def reset(self):
        pass

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


class ClassicReset(ResetStrategy):
    def reset(self):
        self._setDTR(False)  # IO0=HIGH
        self._setRTS(True)  # EN=LOW, chip in reset
        time.sleep(0.1)
        self._setDTR(True)  # IO0=LOW
        self._setRTS(False)  # EN=HIGH, chip out of reset
        time.sleep(self.reset_delay)
        self._setDTR(False)  # IO0=HIGH, done


class UnixTightReset(ResetStrategy):
    def reset(self):
        self._setDTRandRTS(False, False)
        self._setDTRandRTS(True, True)
        self._setDTRandRTS(False, True)  # IO0=HIGH & EN=LOW, chip in reset
        time.sleep(0.1)
        self._setDTRandRTS(True, False)  # IO0=LOW & EN=HIGH, chip out of reset
        time.sleep(self.reset_delay)
        self._setDTRandRTS(False, False)  # IO0=HIGH, done
        self._setDTR(False)  # Needed in some environments to ensure IO0=HIGH


class HardReset(ResetStrategy):
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
