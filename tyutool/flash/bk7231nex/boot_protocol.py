# encoding: utf8
#
# UART Download Tool
#
# Copyright (c) BekenCorp. (chunjian.tian@bekencorp.com).  All rights reserved.
#
# This software may be distributed under the terms of the BSD license.
# See README for more details.

import struct

CMD_LinkCheck=0
CMD_ReadBootVersion = 0x11
CMD_ReadReg=3
CMD_WriteReg=1
CMD_SetBaudRate=0x0f
CMD_CheckCRC=0x10
CMD_Reboot=0x0e
CMD_StayRom=0xaa
CMD_Reset=0xfe
CMD_RESET=0x70
CMD_FlashEraseAll=0x0a
CMD_FlashErase4K=0x0b
CMD_FlashErase=0x0f
CMD_FlashWrite4K=0x07
CMD_FlashRead4K=0x09
CMD_FlashWrite=0x06
CMD_FlashRead=0x08
CMD_FlashReadSR=0x0c
CMD_FlashWriteSR=0x0d
CMD_FlashGetMID=0x0e

# BKReg CMD
CMD_BKREG_DO_REBOOT = 0xFE


def BuildCmd_LinkCheck():
    length = 1
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=length
    buf[4]=CMD_LinkCheck
    return buf[:length+4]

def BuildCmd_ReadBootVersion():
    length=1
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=length
    buf[4]=CMD_ReadBootVersion
    return buf[:length+4]


def BuildCmd_ReadRegn(regAddr):
    length=1+(4)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=length
    buf[4]=CMD_ReadReg
    buf[5]=(regAddr&0xff)
    buf[6]=((regAddr>>8)&0xff)
    buf[7]=((regAddr>>16)&0xff)
    buf[8]=((regAddr>>24)&0xff)
    return buf[:length+4]

def BuildCmd_WriteReg(regAddr: int, val: int):
    length=1+(4+4)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=length
    buf[4]=CMD_WriteReg
    buf[5]=(regAddr&0xff)
    buf[6]=((regAddr>>8)&0xff)
    buf[7]=((regAddr>>16)&0xff)
    buf[8]=((regAddr>>24)&0xff)
    buf[9]=(val&0xff)
    buf[10]=((val>>8)&0xff)
    buf[11]=((val>>16)&0xff)
    buf[12]=((val>>24)&0xff)
    return buf[:length+4]

def BuildCmd_SetBaudRate(baudrate: int, dly_ms: int):
    length=1+(4+1)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=length
    buf[4]=CMD_SetBaudRate
    buf[5]=(baudrate&0xff)
    buf[6]=((baudrate>>8)&0xff)
    buf[7]=((baudrate>>16)&0xff)
    buf[8]=((baudrate>>24)&0xff)
    buf[9]=(dly_ms&0xff)
    return buf[:length+4]


def BuildCmd_CheckCRC(startAddr: int, endAddr: int):
    length=1+(4+4)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=length
    buf[4]=CMD_CheckCRC
    buf[5]=(startAddr&0xff)
    buf[6]=((startAddr>>8)&0xff)
    buf[7]=((startAddr>>16)&0xff)
    buf[8]=((startAddr>>24)&0xff)
    buf[9]=(endAddr&0xff)
    buf[10]=((endAddr>>8)&0xff)
    buf[11]=((endAddr>>16)&0xff)
    buf[12]=((endAddr>>24)&0xff)
    return buf[:length+4]

def BuildCmd_Reboot():
    length=1+(1)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=length
    buf[4]=CMD_Reboot
    buf[5]=0xa5
    return buf[:length+4]

# !BK7236 & !BK7231N
def BuildCmd_Reset():
    length=1+(4)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=length
    buf[4]=CMD_Reset
    buf[5]=0x95
    buf[6]=0x27
    buf[7]=0x95
    buf[8]=0x27
    return buf[:length+4]

# BK7236 & BK7231N
def BuildCmd_RESET():
    length=1+(4)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=length
    buf[4]=CMD_RESET
    buf[5]=0x53
    buf[6]=0x45
    buf[7]=0x41
    buf[8]=0x4E
    return buf[:length+4]

def BuildCmd_StayRom():
    length=1+(1)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=length
    buf[4]=CMD_StayRom
    buf[5]=0x55
    return buf[:length+4]


def BuildCmd_FlashEraseAll():
    length=1+(1)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=0xff
    buf[4]=0xf4
    buf[5]=(length&0xff)
    buf[6]=((length>>8)&0xff)
    buf[7]=CMD_FlashEraseAll
    buf[8]=4
    return buf[:length+7]

def BuildCmd_FlashErase4K(addr: int):
    length=1+(4)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=0xff
    buf[4]=0xf4
    buf[5]=(length&0xff)
    buf[6]=((length>>8)&0xff)
    buf[7]=CMD_FlashErase4K
    buf[8]=(addr&0xff)
    buf[9]=((addr>>8)&0xff)
    buf[10]=((addr>>16)&0xff)
    buf[11]=((addr>>24)&0xff)
    return buf[:length+7]


def BuildCmd_FlashErase(addr: int, szCmd: int):
    length=1+(4+1)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=0xff
    buf[4]=0xf4
    buf[5]=(length&0xff)
    buf[6]=((length>>8)&0xff)
    buf[7]=CMD_FlashErase
    buf[8]=szCmd
    buf[9]=(addr&0xff)
    buf[10]=((addr>>8)&0xff)
    buf[11]=((addr>>16)&0xff)
    buf[12]=((addr>>24)&0xff)
    return buf[:length+7]


def BuildCmd_FlashWrite4K(addr: int, data):
    length=1+(4+4*1024)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=0xff
    buf[4]=0xf4
    buf[5]=(length&0xff)
    buf[6]=((length>>8)&0xff)
    buf[7]=CMD_FlashWrite4K
    buf[8]=(addr&0xff)
    buf[9]=((addr>>8)&0xff)
    buf[10]=((addr>>16)&0xff)
    buf[11]=((addr>>24)&0xff)
    buf[12:12+len(data)+1] = data # len(dat) = 4*1024
    return buf[:length+7]

def BuildCmd_FlashRead4K(addr: int):
    length=1+(4+0)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=0xff
    buf[4]=0xf4
    buf[5]=(length&0xff)
    buf[6]=((length>>8)&0xff)
    buf[7]=CMD_FlashRead4K
    buf[8]=(addr&0xff)
    buf[9]=((addr>>8)&0xff)
    buf[10]=((addr>>16)&0xff)
    buf[11]=((addr>>24)&0xff)
    return buf[:length+7]


def BuildCmd_FlashWrite(addr: int, dat):
    length=1+(4+len(dat))
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=0xff
    buf[4]=0xf4
    buf[5]=(length&0xff)
    buf[6]=((length>>8)&0xff)
    buf[7]=CMD_FlashWrite
    buf[8]=(addr&0xff)
    buf[9]=((addr>>8)&0xff)
    buf[10]=((addr>>16)&0xff)
    buf[11]=((addr>>24)&0xff)
    buf[12:12+len(dat)+1] = dat
    return buf[:length+7]

def BuildCmd_FlashRead(addr: int, lenObj: int):
    length=1+(4+2)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=0xff
    buf[4]=0xf4
    buf[5]=(length&0xff)
    buf[6]=((length>>8)&0xff)
    buf[7]=CMD_FlashWrite4K
    buf[8]=(addr&0xff)
    buf[9]=((addr>>8)&0xff)
    buf[10]=((addr>>16)&0xff)
    buf[11]=((addr>>24)&0xff)
    buf[12]=(lenObj&0xff)
    buf[13]=((lenObj>>8)&0xff)
    return buf[:length+7]

def BuildCmd_FlashReadSR(regAddr: int):
    length=1+(1+0)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=0xff
    buf[4]=0xf4
    buf[5]=(length&0xff)
    buf[6]=((length>>8)&0xff)
    buf[7]=CMD_FlashReadSR
    buf[8]=(regAddr&0xff)
    return buf[:length+7]


def BuildCmd_FlashWriteSR(regAddr: int, val: int):
    length=1+(1+1)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=0xff
    buf[4]=0xf4
    buf[5]=(length&0xff)
    buf[6]=((length>>8)&0xff)
    buf[7]=CMD_FlashWriteSR
    buf[8]=(regAddr&0xff)
    buf[9]=((val)&0xff)
    return buf[:length+7]


def BuildCmd_FlashWriteSR2(regAddr: int, val: int):
    length=1+(1+2)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=0xff
    buf[4]=0xf4
    buf[5]=(length&0xff)
    buf[6]=((length>>8)&0xff)
    buf[7]=CMD_FlashWriteSR
    buf[8]=(regAddr&0xff)
    buf[9]=((val)&0xff)
    buf[10]=((val>>8)&0xff)
    return buf[:length+7]


def BuildCmd_FlashGetMID(regAddr: int):
    length=(1+4)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=0xff
    buf[4]=0xf4
    buf[5]=(length&0xff)
    buf[6]=((length>>8)&0xff)
    buf[7]=CMD_FlashGetMID
    buf[8]=(regAddr&0xff)
    buf[9]=0
    buf[10]=0
    buf[11]=0
    return buf[:length+7]

def BuildCmd_Bkreg_DoReboot():
    length=1+(4)
    buf = bytearray(4096)
    buf[0]=0x01
    buf[1]=0xe0
    buf[2]=0xfc
    buf[3]=length
    buf[4]=CMD_BKREG_DO_REBOOT
    buf[5]=0x95
    buf[6]=0x27
    buf[7]=0x95
    buf[8]=0x27
    return buf[:length+4]

def CheckRespond_LinkCheck(buf):
    cBuf = bytes([0x04,0x0e,0x05,0x01,0xe0,0xfc,CMD_LinkCheck+1,0x00])
    return True if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)] else False


def CheckRespond_BootVersion(buf, versionLen):
    cBuf = bytearray([0x04,0x0e,0x05,0x01,0xe0,0xfc,CMD_ReadBootVersion])
    cBuf[2] = 4 + versionLen
    if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)]:
        return True, buf[len(cBuf):]
    return False, None


def CheckRespond_ReadReg(buf, regAddr):
    cBuf = bytearray([0x04,0x0e,0x05,0x01,0xe0,0xfc,CMD_ReadReg, 0, 0, 0, 0])
    cBuf[2]=3+1+4+4
    cBuf[7]=(regAddr&0xff)
    cBuf[8]=((regAddr>>8)&0xff)
    cBuf[9]=((regAddr>>16)&0xff)
    cBuf[10]=((regAddr>>24)&0xff)
    if len(cBuf) <= len(buf) and cBuf == buf[:11]:
        t=buf[14]
        t=(t<<8)+buf[13]
        t=(t<<8)+buf[12]
        t=(t<<8)+buf[11]
        return True, t
    return False, None

def CheckRespond_WriteReg(buf, regAddr, val):
    cBuf = bytearray([0x04,0x0e,0x05,0x01,0xe0,0xfc,CMD_WriteReg,0,0,0,0,0,0,0,0])
    cBuf[2]=3+1+4+4
    cBuf[7]=(regAddr&0xff)
    cBuf[8]=((regAddr>>8)&0xff)
    cBuf[9]=((regAddr>>16)&0xff)
    cBuf[10]=((regAddr>>24)&0xff)
    cBuf[11]=(val&0xff)
    cBuf[12]=((val>>8)&0xff)
    cBuf[13]=((val>>16)&0xff)
    cBuf[14]=((val>>24)&0xff)

    return True if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)] else False

def CheckRespond_SetBaudRate(buf, baudrate, dly_ms):
    # It seems like multiple people are affected by the baud rate reply
    # containing two concatenated messages, with the one we need (baud rate reply)
    # arriving second. Therefore ignore the unexpected-but-actually-expected
    # message if it's there.
    # https://github.com/OpenBekenIOT/hid_download_py/issues/3
    unexpected = bytearray([0x04, 0x0e, 0x05, 0x01, 0xe0, 0xfc, 0x01, 0x00])
    if buf[:len(unexpected)] == unexpected:
        buf = buf[len(unexpected):]
        print("caution: ignoring unexpected reply in SetBaudRate")
    cBuf =bytearray([0x04,0x0e,0x05,0x01,0xe0,0xfc,CMD_SetBaudRate,0,0,0,0,0])
    cBuf[2]=3+1+4+1
    cBuf[7]=(baudrate&0xff)
    cBuf[8]=((baudrate>>8)&0xff)
    cBuf[9]=((baudrate>>16)&0xff)
    cBuf[10]=((baudrate>>24)&0xff)
    cBuf[11]=(dly_ms&0xff)

    return True if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)] else False

def CheckRespond_CheckCRC(buf, startAddr, endAddr):
    cBuf = bytearray([0x04,0x0e,0x05,0x01,0xe0,0xfc,CMD_CheckCRC])
    cBuf[2]=3+1+4
    # FIXME: Length check
    if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)]:
        t=buf[10]
        t=(t<<8)+buf[9]
        t=(t<<8)+buf[8]
        t=(t<<8)+buf[7]
        return True, t
    return False, 0

def CheckRespond_StayRom(buf):
    cBuf = bytearray([0x04,0x0e,0x05,0x01,0xe0,0xfc,CMD_StayRom])
    cBuf[2]=3+1+1
    if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)]:
        if buf[7]==0x55:
            return True
    return False

def CheckRespond_SysErr(buf):
    cBuf = bytearray([0x04,0x0e,0x04,0x01,0xe0,0xfc])
    if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)]:
        if buf[6]==0xee:
            return 1
        elif buf[6]==0xfe:
            return 2
    return False

def CheckRespond_FlashEraseAll(buf, status, to_s):
    cBuf = bytearray([0x04,0x0e,0xff,0x01,0xe0,0xfc,0xf4,0x03,0x00,CMD_FlashEraseAll])
    if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)]:
        return True, buf[10], buf[11]
    return False,None,None

def CheckRespond_FlashErase4K(buf, addr):
    cBuf = bytearray([0x04,0x0e,0xff,0x01,0xe0,0xfc,0xf4,0x06,0x00,CMD_FlashErase4K])
    if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)]:
        # TODO memcmp(&buf[11],&addr,4)==0
        return True, buf[10]
    return False

def CheckRespond_FlashErase(buf, addr, szCmd):
    cBuf = bytearray([0x04,0x0e,0xff,0x01,0xe0,0xfc,0xf4,1+1+(1+4), 0x00,CMD_FlashErase])
    if len(cBuf) <= len(buf) and buf[11] == szCmd and cBuf == buf[:len(cBuf)]:
        # TODO: memcmp(&buf[12],&addr,4)==0
        return True, buf[10]
    return False, None

def CheckRespond_FlashWrite4K(buf, addr):
    cBuf = bytearray([0x04,0x0e,0xff,0x01,0xe0,0xfc,0xf4,1+1+(4),0x00,CMD_FlashWrite4K])
    if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)]:
        # TODO: memcmp(&buf[11],&addr,4)==0
        return True, buf[10]
    return False, None

def CheckRespond_FlashRead4K(buf, addr):
    '''
    return operation_status, status, buf
    '''
    cBuf = bytearray([0x04,0x0e,0xff,0x01,0xe0,0xfc,0xf4,(1+1+(4+4*1024))&0xff,
        ((1+1+(4+4*1024))>>8)&0xff,CMD_FlashRead4K])
    if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)]:
        # TODO: memcmp(&buf[11],&addr,4)==0
        return True, buf[10], buf[15:]
    return False, None, None

def CheckRespond_FlashWrite(buf, addr):
    cBuf = bytearray([0x04,0x0e,0xff,0x01, 0xe0,0xfc,0xf4,(1+1+(4+1))&0xff, ((1+1+(4+1))>>8)&0xff,CMD_FlashWrite])
    if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)]:
        # TODO: memcmp(&buf[11],&addr,4)==0
        return True, buf[10], buf[15]
    return False, None, None

def CheckRespond_FlashRead(buf, addr, lenObj):
    cBuf = bytearray([0x04,0x0e,0xff,0x01, 0xe0,0xfc,0xf4,(1+1+(4+lenObj))&0xff, ((1+1+(4+lenObj))>>8)&0xff,CMD_FlashRead])
    if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)]:
        # TODO: memcmp(&buf[11],&addr,4)==0
        return True, buf[10], buf[15:]
    return False, None, None

def CheckRespond_FlashReadSR(buf, regAddr):
    cBuf = bytearray([0x04,0x0e,0xff,0x01, 0xe0,0xfc,0xf4,(1+1+(1+1))&0xff, ((1+1+(1+1))>>8)&0xff,CMD_FlashReadSR])
    if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)] and regAddr == buf[11]:
        return True, buf[10], buf[12]
    return False, 0, 0

def CheckRespond_FlashWriteSR(buf, regAddr, val):
    cBuf = bytearray([0x04,0x0e,0xff,0x01,
        0xe0,0xfc,0xf4,(1+1+(1+1))&0xff,
        ((1+1+(1+1))>>8)&0xff,CMD_FlashWriteSR])
    # print("writeSR: ", buf)
    if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)] and val == buf[12] and regAddr == buf[11]:
        return True, buf[10]
    return False, None

def CheckRespond_FlashWriteSR2(buf, regAddr, val):
    cBuf = bytearray([0x04,0x0e,0xff,0x01,
        0xe0,0xfc,0xf4,(1+1+(1+2))&0xff,
        ((1+1+(1+2))>>8)&0xff,CMD_FlashWriteSR])
    # print("writeSR2: ", buf)
    if len(cBuf) <= len(buf) and cBuf == buf[:len(cBuf)] and val&0xFF == buf[12] and ((val>>8)&0xFF) == buf[13]:
        return True, buf[10]
    return False, None

def CheckRespond_FlashGetMID(buf):
    cBuf = bytearray([0x04,0x0e,0xff,0x01,
        0xe0,0xfc,0xf4,(1+4)&0xff,
        ((1+4)>>8)&0xff,CMD_FlashGetMID])
    if len(cBuf) <= len(buf) and cBuf == buf[:10]:
        return struct.unpack("<I", buf[11:])[0]>>8
        # return True, buf[10], struct.unpack("<I", buf[11:])[0]>>8

    # FIX BootROM Bug
    cBuf[7] += 1
    if cBuf == buf[:10]:
        return struct.unpack("<I", buf[11:])[0]>>8
        # return True, buf[10], struct.unpack("<I", buf[11:])[0]>>8

    return 0

def GetRespond_CmdType(buf):
    return buf[9] if buf[2] == 0xff else buf[6]


def CalcRxLength_LinkCheck():
    return(3+3+1+1+0)

def CalcRxLength_BootVersion(version_len):
    return(3+3+1+version_len)

def CalcRxLength_ReadReg():
    return(3+3+1+4+4)

def CalcRxLength_WriteReg():
    return(3+3+1+4+4)

def CalcRxLength_SetBaudRate():
    return(3+3+1+4+1)

def CalcRxLength_CheckCRC():
    return(3+3+1+4)

def CalcRxLength_StayRom():
    return(3+3+1+1)

def CalcRxLength_SysErr():
    return(3+3+1)

def CalcRxLength_FlashEraseAll():
    return(3+3+3+(1+1+(1+0)))

def CalcRxLength_FlashErase4K():
    return(3+3+3+(1+1+(4+0)))

def CalcRxLength_FlashErase():
    return(3+3+3+(1+1+(1+4)))

def CalcRxLength_FlashWrite4K():
    return(3+3+3+(1+1+(4+0)))

def CalcRxLength_FlashRead4K():
    return(3+3+3+(1+1+(4+4*1024)))

def CalcRxLength_FlashWrite():
    return(3+3+3+(1+1+(4+1)))

def CalcRxLength_FlashRead(lenObj):
    return(3+3+3+(1+1+(4+lenObj)))

def CalcRxLength_FlashReadSR():
    return(3+3+3+(1+1+(1+1)))

def CalcRxLength_FlashWriteSR():
    return(3+3+3+(1+1+(1+1)))

def CalcRxLength_FlashWriteSR2():
    return(3+3+3+(1+1+(1+2)))

def CalcRxLength_FlashGetID():
    return(3+3+3+(1+1+(4)))

