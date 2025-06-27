#!/usr/bin/env python
# -*- coding: utf-8 -*-
#
# Reference https://github.com/tiancj/hid_download_py.git

import struct


class CMD(object):
    LinkCheck       = 0x00
    WriteReg        = 0x01
    ReadReg         = 0x03
    FlashWrite      = 0x06
    FlashWrite4K    = 0x07
    FlashRead       = 0x08
    FlashRead4K     = 0x09
    CheckCRC        = 0x10
    ReadBootVersion = 0x11
    FlashEraseAll   = 0x0a
    FlashErase4K    = 0x0b
    FlashReadSR     = 0x0c
    FlashWriteSR    = 0x0d
    FlashGetMID     = 0x0e
    Reboot          = 0x0e
    FlashErase      = 0x0f
    SetBaudRate     = 0x0f
    RESET           = 0x70
    StayRom         = 0xaa
    Reset           = 0xfe
    BKRegDoReboot   = 0xfe



class BUF(object):
    def __init__(self):
        pass

    @classmethod
    def Reboot(self) -> bytearray:
        length=1+(1)
        buf = bytearray(4096)
        buf[0]=0x01
        buf[1]=0xe0
        buf[2]=0xfc
        buf[3]=length
        buf[4]=CMD.Reboot
        buf[5]=0xa5
        return buf[:length+4]

    @classmethod
    def BKRegDoReboot(self) -> bytearray:
        length=1+(4)
        buf = bytearray(4096)
        buf[0]=0x01
        buf[1]=0xe0
        buf[2]=0xfc
        buf[3]=length
        buf[4]=CMD.BKRegDoReboot
        buf[5]=0x95
        buf[6]=0x27
        buf[7]=0x95
        buf[8]=0x27
        return buf[:length+4]

    @classmethod
    def LinkCheck(self) -> bytearray:
        length = 1
        buf = bytearray(4096)
        buf[0]=0x01
        buf[1]=0xe0
        buf[2]=0xfc
        buf[3]=length
        buf[4]=CMD.LinkCheck
        return buf[:length+4]

    @classmethod
    def SetBaudRate(self, baudrate: int, dly_ms: int) -> bytearray:
        length=1+(4+1)
        buf = bytearray(4096)
        buf[0]=0x01
        buf[1]=0xe0
        buf[2]=0xfc
        buf[3]=length
        buf[4]=CMD.SetBaudRate
        buf[5]=(baudrate&0xff)
        buf[6]=((baudrate>>8)&0xff)
        buf[7]=((baudrate>>16)&0xff)
        buf[8]=((baudrate>>24)&0xff)
        buf[9]=(dly_ms&0xff)
        return buf[:length+4]

    @classmethod
    def FlashRead4K(self, addr: int) -> bytearray:
        length=1+(4+0)
        buf = bytearray(4096)
        buf[0]=0x01
        buf[1]=0xe0
        buf[2]=0xfc
        buf[3]=0xff
        buf[4]=0xf4
        buf[5]=(length & 0xff)
        buf[6]=((length>>8) & 0xff)
        buf[7]=CMD.FlashRead4K
        buf[8]=(addr & 0xff)
        buf[9]=((addr>>8) & 0xff)
        buf[10]=((addr>>16) & 0xff)
        buf[11]=((addr>>24) & 0xff)
        return buf[:length+7]

    @classmethod
    def FlashErase(self, addr: int, szCmd: int) -> bytearray:
        length=1+(4+1)
        buf = bytearray(4096)
        buf[0]=0x01
        buf[1]=0xe0
        buf[2]=0xfc
        buf[3]=0xff
        buf[4]=0xf4
        buf[5]=(length&0xff)
        buf[6]=((length>>8)&0xff)
        buf[7]=CMD.FlashErase
        buf[8]=szCmd
        buf[9]=(addr&0xff)
        buf[10]=((addr>>8)&0xff)
        buf[11]=((addr>>16)&0xff)
        buf[12]=((addr>>24)&0xff)
        return buf[:length+7]

    @classmethod
    def FlashWrite4K(self, addr: int, data: bytearray) -> bytearray:
        length=1+(4+4*1024)
        buf = bytearray(4096)
        buf[0]=0x01
        buf[1]=0xe0
        buf[2]=0xfc
        buf[3]=0xff
        buf[4]=0xf4
        buf[5]=(length&0xff)
        buf[6]=((length>>8)&0xff)
        buf[7]=CMD.FlashWrite4K
        buf[8]=(addr&0xff)
        buf[9]=((addr>>8)&0xff)
        buf[10]=((addr>>16)&0xff)
        buf[11]=((addr>>24)&0xff)
        buf[12:12+len(data)+1] = data # len(dat) = 4*1024
        return buf[:length+7]

    @classmethod
    def CheckCRC(self, startAddr: int, endAddr: int) -> bytearray:
        length=1+(4+4)
        buf = bytearray(4096)
        buf[0]=0x01
        buf[1]=0xe0
        buf[2]=0xfc
        buf[3]=length
        buf[4]=CMD.CheckCRC
        buf[5]=(startAddr&0xff)
        buf[6]=((startAddr>>8)&0xff)
        buf[7]=((startAddr>>16)&0xff)
        buf[8]=((startAddr>>24)&0xff)
        buf[9]=(endAddr&0xff)
        buf[10]=((endAddr>>8)&0xff)
        buf[11]=((endAddr>>16)&0xff)
        buf[12]=((endAddr>>24)&0xff)
        return buf[:length+4]

    @classmethod
    def FlashGetMID(self, regAddr: int):
        length=(1+4)
        buf = bytearray(4096)
        buf[0]=0x01
        buf[1]=0xe0
        buf[2]=0xfc
        buf[3]=0xff
        buf[4]=0xf4
        buf[5]=(length&0xff)
        buf[6]=((length>>8)&0xff)
        buf[7]=CMD.FlashGetMID
        buf[8]=(regAddr&0xff)
        buf[9]=0
        buf[10]=0
        buf[11]=0
        return buf[:length+7]

    @classmethod
    def FlashReadSR(self, regAddr: int):
        length=1+(1+0)
        buf = bytearray(4096)
        buf[0]=0x01
        buf[1]=0xe0
        buf[2]=0xfc
        buf[3]=0xff
        buf[4]=0xf4
        buf[5]=(length&0xff)
        buf[6]=((length>>8)&0xff)
        buf[7]=CMD.FlashReadSR
        buf[8]=(regAddr&0xff)
        return buf[:length+7]

    @classmethod
    def FlashWriteSR(self, regAddr: int, val: int):
        length=1+(1+1)
        buf = bytearray(4096)
        buf[0]=0x01
        buf[1]=0xe0
        buf[2]=0xfc
        buf[3]=0xff
        buf[4]=0xf4
        buf[5]=(length&0xff)
        buf[6]=((length>>8)&0xff)
        buf[7]=CMD.FlashWriteSR
        buf[8]=(regAddr&0xff)
        buf[9]=((val)&0xff)
        return buf[:length+7]

    @classmethod
    def FlashWriteSR2(self, regAddr: int, val: int):
        length=1+(1+2)
        buf = bytearray(4096)
        buf[0]=0x01
        buf[1]=0xe0
        buf[2]=0xfc
        buf[3]=0xff
        buf[4]=0xf4
        buf[5]=(length&0xff)
        buf[6]=((length>>8)&0xff)
        buf[7]=CMD.FlashWriteSR
        buf[8]=(regAddr&0xff)
        buf[9]=((val)&0xff)
        buf[10]=((val>>8)&0xff)
        return buf[:length+7]


    @classmethod
    def CheckRespond_LinkCheck(self, buf: bytearray) -> bool:
        cBuf = bytes([0x04, 0x0e, 0x05, 0x01, 0xe0, 0xfc, CMD.LinkCheck+1, 0x00])
        return True if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)] else False


    @classmethod
    def CheckRespond_SetBaudRate(self,
                                 buf: str,
                                 baudrate: int,
                                 dly_ms: int) -> bool:
        # It seems like multiple people are affected by the baud rate reply
        # containing two concatenated messages, with the one we need (baud rate reply)
        # arriving second. Therefore ignore the unexpected-but-actually-expected
        # message if it's there.
        # https://github.com/OpenBekenIOT/hid_download_py/issues/3
        unexpected = bytearray([0x04, 0x0e, 0x05, 0x01, 0xe0, 0xfc, 0x01, 0x00])
        if buf[:len(unexpected)] == unexpected:
            buf = buf[len(unexpected):]
            print("caution: ignoring unexpected reply in SetBaudRate")
        cBuf =bytearray([0x04,0x0e,0x05,0x01,0xe0,0xfc,CMD.SetBaudRate,0,0,0,0,0])
        cBuf[2]=3+1+4+1
        cBuf[7]=(baudrate&0xff)
        cBuf[8]=((baudrate>>8)&0xff)
        cBuf[9]=((baudrate>>16)&0xff)
        cBuf[10]=((baudrate>>24)&0xff)
        cBuf[11]=(dly_ms&0xff)
        return True if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)] else False


    @classmethod
    def CheckRespond_FlashRead4K(self,
                                 buf: bytearray,
                                 addr: int) -> (bool, bytearray, bytearray):
        '''
        return operation_status, status, buf
        '''
        cBuf = bytearray([0x04,0x0e,0xff,0x01,0xe0,0xfc,0xf4,(1+1+(4+4*1024))&0xff,
            ((1+1+(4+4*1024))>>8)&0xff,CMD.FlashRead4K])
        if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)]:
            return True, buf[10], buf[15:]
        return False, b'', b''


    @classmethod
    def CheckRespond_FlashErase(self,
                                buf: bytearray,
                                addr: int,
                                szCmd: int) -> (bool, bytearray):
        cBuf = bytearray([0x04,0x0e,0xff,0x01,0xe0,0xfc,0xf4,1+1+(1+4), 0x00,CMD.FlashErase])
        if len(cBuf) <= len(buf) and buf[11] == szCmd and cBuf == buf[:len(cBuf)]:
            # TODO: memcmp(&buf[12],&addr,4)==0
            return True, buf[10]
        return False, b''


    @classmethod
    def CheckRespond_FlashWrite4K(self, buf: bytearray, addr: int) -> bool:
        cBuf = bytearray([0x04,0x0e,0xff,0x01,0xe0,0xfc,0xf4,1+1+(4),0x00,CMD.FlashWrite4K])
        if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)]:
            # TODO: memcmp(&buf[11],&addr,4)==0
            return True, buf[10]
        return False, b''


    @classmethod
    def CheckRespond_CheckCRC(self,
                              buf: bytearray,
                              startAddr: int,
                              endAddr: int) -> (bool, int):
        cBuf = bytearray([0x04,0x0e,0x05,0x01,0xe0,0xfc,CMD.CheckCRC])
        cBuf[2]=3+1+4
        # FIXME: Length check
        if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)]:
            t=buf[10]
            t=(t<<8)+buf[9]
            t=(t<<8)+buf[8]
            t=(t<<8)+buf[7]
            return True, t
        return False, 0

    @classmethod
    def CheckRespond_FlashGetMID(self, buf: bytearray):
        cBuf = bytearray([0x04,0x0e,0xff,0x01,
            0xe0,0xfc,0xf4,(1+4)&0xff,
            ((1+4)>>8)&0xff,CMD.FlashGetMID])
        if len(buf) != 15:
            return 0
        if len(cBuf) <= len(buf) and cBuf == buf[:10]:
            return struct.unpack("<I", buf[11:])[0]>>8
            # return True, buf[10], struct.unpack("<I", buf[11:])[0]>>8
        # FIX BootROM Bug
        cBuf[7] += 1
        if cBuf == buf[:10]:
            return struct.unpack("<I", buf[11:])[0]>>8
            # return True, buf[10], struct.unpack("<I", buf[11:])[0]>>8
        return 0

    @classmethod
    def CheckRespond_FlashWriteSR(self, buf, regAddr, val):
        cBuf = bytearray([0x04,0x0e,0xff,0x01,
            0xe0,0xfc,0xf4,(1+1+(1+1))&0xff,
            ((1+1+(1+1))>>8)&0xff,CMD.FlashWriteSR])
        # print("writeSR: ", buf)
        if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)] and val == buf[12] and regAddr == buf[11]:
            return True, buf[10]
        return False, None

    @classmethod
    def CheckRespond_FlashWriteSR2(self, buf, regAddr, val):
        cBuf = bytearray([0x04,0x0e,0xff,0x01,
            0xe0,0xfc,0xf4,(1+1+(1+2))&0xff,
            ((1+1+(1+2))>>8)&0xff,CMD.FlashWriteSR])
        # print("writeSR2: ", buf)
        if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)] and val&0xFF == buf[12] and ((val>>8)&0xFF) == buf[13]:
            return True, buf[10]
        return False, None

    @classmethod
    def CheckRespond_FlashReadSR(self, buf: bytearray, regAddr: int):
        cBuf = bytearray([0x04,0x0e,0xff,0x01, 0xe0,0xfc,0xf4,(1+1+(1+1))&0xff, ((1+1+(1+1))>>8)&0xff,CMD.FlashReadSR])
        if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)] and regAddr == buf[11]:
            return True, buf[10], buf[12]
        return False, 0, 0

    @classmethod
    def CalcRxLength_LinkCheck(self):
        return 8  # (3+3+1+1+0)

    @classmethod
    def CalcRxLength_SetBaudRate(self):
        return 12  # (3+3+1+4+1)

    @classmethod
    def CalcRxLength_FlashRead4K(self):
        return 4111  # (3+3+3+(1+1+(4+4*1024)))

    @classmethod
    def CalcRxLength_FlashErase(self):
        return 16  # (3+3+3+(1+1+(1+4)))

    @classmethod
    def CalcRxLength_FlashWrite4K(self):
        return 15  # (3+3+3+(1+1+(4+0)))

    @classmethod
    def CalcRxLength_CheckCRC(self):
        return 11  # (3+3+1+4)

    @classmethod
    def CalcRxLength_FlashGetID(self):
        return 15  # (3+3+3+(1+1+(4)))

    @classmethod
    def CalcRxLength_FlashReadSR(self):
        return 13  # (3+3+3+(1+1+(1+1)))

    @classmethod
    def CalcRxLength_FlashWriteSR(self):
        return 13  # (3+3+3+(1+1+(1+1)))

    @classmethod
    def CalcRxLength_FlashWriteSR2(self):
        return 14  # (3+3+3+(1+1+(1+2)))


class CRC(object):
    def __init__(self):
        # make crc32 table
        crc32_table = []
        for i in range(0,256):
            c = i
            for j in range(0,8):
                if c&1:
                    c = 0xEDB88320 ^ (c >> 1)
                else:
                    c = c >> 1
            crc32_table.append(c)
        self.crc32_table = crc32_table
        pass

    def crc32(self, crc, buf):
        for c in buf:
            crc = (crc>>8) ^ self.crc32_table[(crc^c)&0xff]
        return crc


class FLASH(object):
    class ID(object):
        XTX_25F08B=0x14405e        # 芯天下flash,w+v:6.5s,e+w+v:11.5s
        XTX_25F04B=0x13311c        # 芯天下flash-4M
        XTX_25F16B=0x15400b        # XTX 16M
        XTX_25F32B=0x0016400b      # xtx 32M******暂时只用于脱机烧录器上7231的外挂flash
        XTX_25Q64B=0x0017600b      # xtx 64M******暂时只用于脱机烧录器上7231的外挂flash
        XTX_25F64B=0x0017400b      # xtx 64M******暂时只用于脱机烧录器上7231的外挂flash
        MXIC_25V8035F=0x1423c2     # 旺宏flash,w+v:8.2s,e+w+v:17.2s
        MXIC_25V4035F=0x1323c2     # 旺宏flash,w+v:8.2s,e+w+v:17.2s
        MXIC_25V1635F=0x1523c2     # 旺宏flash,w+v:8.2s,e+w+v:17.2s
        GD_25D40=0x134051          # GD flash-4M,w+v:3.1s,e+w+v:5.1s
        GD_25D80=0x144051          # GD flash-8M，e+w+v=9.8s
        GD_1_25D80=0x1440C8        # GD flash-8M，
        GD_25WQ64E=0x001765c8
        GD_25WQ32E=0x001665c8
        GD_25WQ16E=0x001565c8
        GD_25Q64=0x001740c8
        GD_25Q16=0x001540c8        # GD 16M******暂时只用于脱机烧录器上7231的外挂flash
        GD_25Q16B=0x001540c8       # GD 16M******暂时只用于脱机烧录器上7231的外挂flash
        GD_25Q41B=0x1340c8         # GD flash-4M,w+v:3.1s,e+w+v:5.1s
        Puya_25Q16HB_K=0x152085
        Puya_25Q40=0x136085        # puya 4M,e+w+v:6s，新版w+v=4s，e+w+v=4.3s
        Puya_25Q64H=0x00176085
        Puya_25Q80=0x146085        # puya 8M,w+v:10.4s,e+w+v:11.3s,新版e+w+v：8.3s
        Puya_25Q80_38=0x154285
        Puya_25Q32H=0x166085       # puya 32M******暂时只用于脱机烧录器上7231的外挂flash
        BY_PN25Q80A=0x1440e0       # GD flash-4M,w+v:3.1s,e+w+v:5.1s
        BY_PN25Q40A=0x1340e0       # GD flash-4M,w+v:3.1s,e+w+v:5.1s
        WB_25Q128JV=0x001840ef
        ESMT_25QH16B=0x0015701c
        ESMT_25QH32A=0x0016411c
        ESMT_25QW32A=0x0016611c
        TH25Q_16HB = 0x001560eb
        TH25Q_80HB = 0x001460cd
        NA = 0x001640c8
        UNKNOWN=-1
        pass

    # Struct
    class Int(object):
        def __init__(self, mid, icNam, manName, szMem, szSR,
                cwUnp, cwEnp, cwMsk, sb, lb, cwdRd, cwdWr):
            self.mid = mid
            self.icNam = icNam
            self.manName = manName
            self.szMem = szMem
            self.szSR = szSR
            self.cwUnp = cwUnp
            self.cwEnp = cwEnp
            self.cwMsk = cwMsk
            self.sb = sb
            self.lb = lb
            self.cwdRd = cwdRd
            self.cwdWr = cwdWr
            pass

        def __str__(self):
            return f"mid: {self.mid}, name: {self.icNam}, manufactor: {self.manName}, size: {self.szMem}"

    @classmethod
    def CalcBFD(self, v,bs,bl):
        return (v&((1<<(bl))-1))<<(bs)

    def BFD(v,bs,bl):
        return (v&((1<<(bl))-1))<<(bs)

    def BIT(n):
        return 1<<n

    TABLE = [
        #    MID                 IC Name         manufactor     size        # SR  unprot    prot       mask              sb    length
        Int(ID.XTX_25F08B,     "PN25F08B",     "xtx",      8 *1024*1024,   1,    0x00,    0x07,   BFD(0x0f,2,4),           2,    4,    [0x05,0xff,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.XTX_25F04B,     "PN25F04B",     "xtx",      4 *1024*1024,   1,    0x00,    0x07,   BFD(0x0f,2,4),           2,    4,    [0x05,0xff,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.GD_25D40,       "GD25D40",      "GD",       4 *1024*1024,   1,    0x00,    0x07,   BFD(0x0f,2,3),           2,    3,    [0x05,0xff,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.GD_25D80,       "GD25D80",      "GD",       8 *1024*1024,   1,    0x00,    0x07,   BFD(0x0f,2,3),           2,    3,    [0x05,0xff,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.GD_1_25D80,     "GD25D80",      "GD",       8 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.Puya_25Q80,     "P25Q80",       "Puya",     8 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.Puya_25Q80_38,  "P25Q80",       "Puya",    16 *1024*1024,   1,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.Puya_25Q16HB_K, "P25Q16HB_K",   "Puya",    16 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.Puya_25Q40,     "P25Q40",       "Puya",     4 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.Puya_25Q32H,    "P25Q32H",      "Puya",    32 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.Puya_25Q64H,    "P25Q64H",      "Puya",    64 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.XTX_25F16B,     "XT25F16B",     "xtx",     16 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.GD_25Q16B,      "GD25Q16B",     "GD",      16 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.MXIC_25V8035F,  "MX25V8035F",   "WH",       8 *1024*1024,   2,    0x00,    0x07,   BIT(12)|BFD(0x1f,2,4),   2,    5,    [0x05,0x15,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.MXIC_25V1635F,  "MX25V1635F",   "WH",      16 *1024*1024,   2,    0x00,    0x07,   BIT(12)|BFD(0x1f,2,4),   2,    5,    [0x05,0x15,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.XTX_25F32B,     "XT25F32B",     "xtx",     32 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.GD_25Q41B,      "GD25Q41B",     "GD",       4 *1024*1024,   1,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,3),   2,    3,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.BY_PN25Q40A,    "PN25Q40A",     "BY",       4 *1024*1024,   1,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,3),   2,    3,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.BY_PN25Q80A,    "PN25Q80A",     "BY",       8 *1024*1024,   1,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,3),   2,    3,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.XTX_25F64B,     "XT25F64B",     "xtx",     64 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.XTX_25Q64B,     "XT25Q64B",     "xtx",     64 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.WB_25Q128JV,    "WB25Q128JV",   "WB",     128 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.ESMT_25QH16B,   "EN25QH16B",    "ESMT",    16 *1024*1024,   1,    0x00,    0x07,   BFD(0xf,2,5),            2,    4,    [0x05,0xff,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.ESMT_25QH32A,   "EN25QH32A",    "ESMT",    32 *1024*1024,   1,    0x00,    0x07,   BFD(0xf,2,5),            2,    4,    [0x05,0xff,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.ESMT_25QW32A,   "EN25QH32A",    "ESMT",    32 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.TH25Q_16HB,     "TH25Q_16HB",   "TH",      16 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.TH25Q_80HB,     "TH25Q_80HB",   "TH",       8 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.NA,             "NA_NA",        "NA",      32 *1024*1024,   1,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.GD_25WQ16E,     "GD25WQ16E",    "GD",      16 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.GD_25WQ32E,     "GD25WQ32E",    "GD",      32 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
        Int(ID.GD_25WQ64E,     "GD25WQ64E",    "GD",      64 *1024*1024,   2,    0x00,    0x07,   BIT(14)|BFD(0x1f,2,5),   2,    5,    [0x05,0x35,0xff,0xff],   [0x01,0xff,0xff,0xff]),
    ]

    @classmethod
    def GetFlashInfo(self, mid:int):
        for item in self.TABLE:
            if item.mid == mid:
                return item
        return None

