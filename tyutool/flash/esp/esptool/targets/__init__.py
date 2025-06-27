from .esp32 import ESP32ROM
from .esp32c3 import ESP32C3ROM
from .esp32s3 import ESP32S3ROM

from .stub_flasher.stub_flasher_32 import ESP32STUB
from .stub_flasher.stub_flasher_32c3 import ESP32C3STUB
from .stub_flasher.stub_flasher_32s3 import ESP32S3STUB


CHIP_DEFS = {
    "esp32": ESP32ROM,
    "esp32c3": ESP32C3ROM,
    "esp32s3": ESP32S3ROM,
}

CHIP_LIST = list(CHIP_DEFS.keys())
ROM_LIST = list(CHIP_DEFS.values())

STUB_DEFS = {
    "esp32": ESP32STUB,
    "esp32c3": ESP32C3STUB,
    "esp32s3": ESP32S3STUB,
}
