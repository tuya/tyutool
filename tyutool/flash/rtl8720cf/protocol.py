import struct
import binascii

class CMD(object):
    pass
    
class BUF(object):
    def __init__(self):
        pass

    @classmethod
    def SetModule(self) -> (bytearray):
        length = 10 + 13
        tx_buf = bytearray(128)
        tx_buf[0:length] = b'Rtk8710C\x0D\x0ADW 40000038\x0D\x0A'

        return (tx_buf[:length])
  
    # 发送ping
    @classmethod
    def SendPing(self) -> (bytearray, bytearray):
        tx_buf = bytearray(128)
        length = 5
        tx_buf[0:length] = b'ping\x0A'

        respond_len = 4
        respond = bytearray(128)
        respond[:respond_len] = b'ping'
        
        return (tx_buf[:length], respond[:respond_len])  
    
    @classmethod
    def SetEW(self) -> (bytearray, bytearray):
        length = 10 + 13
        tx_buf = bytearray(128)
        tx_buf[0:length] = b'EW 40002800 7EFFFFFF\x0A'

        respond = bytearray(128)
        respond_len = 25
        respond[:respond_len] = b'0x40002800 = 0x7EFFFFFF\x0D\x0A'            #30 78 34 30 30 30 32 38 30 30 20 3D 20 30 78 37 45 46 46 46 46 46 46 0D 0A 

        return (tx_buf[:length], respond[:respond_len])  
    
    # ucfg 2000000 0 0  
    @classmethod
    def SetBaudRate(self, baudrate: int) -> (bytearray, bytearray):
        tx_buf = bytearray(128)
        # tx_buf[:length] = b'ucfg 2000000 0 0\x0A'
        length = 5
        tx_buf[0:length] = b'ucfg '
        
        #baudrate
        str_baudrate = str(baudrate)
        tx_buf[length:length + len(str_baudrate)] = str_baudrate.encode()

        length += len(str_baudrate)

        tx_buf[length:length + 5] = b' 0 0\x0A'
        length += 5

        respond_len = 2
        respond = bytearray(128)
        respond[0:respond_len] = b'OK'
        return (tx_buf[:length], respond[:respond_len])
    
    # fwd 0 1 
    @classmethod
    def FlashWrite_Start(self, chip_type) -> (bytearray, bytearray):
        tx_buf = bytearray(128)
        length = 9
        if chip_type == 1:                            # RTL8720CF
            tx_buf[0:length] = b'fwd 0 1 \x0A'
        elif chip_type == 2:                          # RTL8720CM
            tx_buf[0:length] = b'fwd 0 0 \x0A'

        respond_len = 1
        respond = bytearray(128)
        respond[0:respond_len] = b'\x15'
        return (tx_buf[:length], respond[:respond_len])
    
    @classmethod
    def FlashWrite_1K(self, index: int, data: bytearray) -> (bytearray, bytearray):
        length = 2 + 1024 + 2
        
        tx_buf = bytearray(length)
        tx_buf[0] = 0x02                   # STX
        tx_buf[1] = index % 0x100          # index
        tx_buf[2] = 0xff - tx_buf[1]
        tx_buf[3:3+len(data)] = data       # [2:1026]

        check_sum = 0
        for v in data:
            check_sum += v
        check_sum %= 0x100

        tx_buf[3+len(data)] = check_sum

        respond_len = 1
        respond = bytearray(128)
        respond[0:respond_len] = b'\x06'

        return tx_buf[:length], respond[:respond_len]

    @classmethod
    def FlashWrite_End(self) -> (bytearray, bytearray):
        tx_buf = bytearray(1)
        length = 1
        tx_buf[0:length] = b'\x04'
 
        respond_len = 3
        respond = bytearray(128)
        respond[0:respond_len] = b'\x06OK'
        return tx_buf[:length], respond[:respond_len]
    

    @classmethod
    def FlashGet_Hash(self, data_len: int) ->(bytearray, bytearray):
        tx_buf = bytearray(128)
        # tx_buf[:length] = b'hashq 1770244 0 1\n'
        length = 6
        tx_buf[0:length] = b'hashq '
        
        #baudrate
        str_data_len = str(data_len)
        tx_buf[length:length + len(str_data_len)] = str_data_len.encode()

        length += len(str_data_len)

        tx_buf[length:length + 5] = b' 0 1\x0A'
        length += 5

        respond_len = 6
        respond = bytearray(128)
        respond[0:respond_len] = b'hashs '
        return tx_buf[:length], respond[:respond_len]