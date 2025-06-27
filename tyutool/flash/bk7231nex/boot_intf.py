# encoding: utf8
#
# UART Download Tool
#
# Copyright (c) BekenCorp. (chunjian.tian@bekencorp.com).  All rights reserved.
#
# This software may be distributed under the terms of the BSD license.
# See README for more details.

# uart wrapper
import serial
from serial import Timeout
from .boot_protocol import *
import binascii
import time

debug = False

RECV_HEAD = 0
RECV_BODY = 1

class CBootIntf(object):
    '''
    XXX: This class is timing sensitive.
    '''
    def __init__(self, port, baudrate, timeout):
        self.ser = serial.Serial(port, baudrate, timeout=timeout)
        self._state = RECV_HEAD

    def Start_Cmd(self, txbuf, rxLen=0, timeout=0.05):
        if debug:
            # print("TX: ", binascii.b2a_hex(txbuf if len(txbuf) < 16 else txbuf[:16], b' '))
            print("TX: ", binascii.b2a_hex(txbuf))
        self.rxLen = rxLen
        if debug:
            print("tx start: ", time.time())
        self.ser.write(txbuf)
        if debug:
            print("tx end: ", time.time())

        if rxLen:
            if debug:
                print("read start: ", time.time())

            rxbuf = self.WaitForRespond(rxLen, timeout)
            if debug and rxbuf:
                print("read end: ", time.time())
                print('RX1: ', binascii.b2a_hex(rxbuf[0:32]))
                #print('RXS: ', rxbuf)
                print('rxLen: ', rxLen)
                print('bufLen: ', len(rxbuf))
            # if rxbuf and (len(rxbuf) == rxLen):
            #   return rxbuf
            return rxbuf
        return None

    def Drain(self):
        # self.ser.timeout = 0.01
        self.ser.read(1024)

    def Read(self):
        return self.ser.read(1024)

    # 04 0e 05 01 e0 fc 01 00
    # 04 0e ff 01 e0 fc f4 07 00 0f 00 20 00 c0 03 00
    def WaitForRespond(self, rxLen, timeout):
        timeout = Timeout(timeout)
        read_buf = b''
        state = RECV_HEAD

        while not timeout.expired():
            buf = self.ser.read(1024)
            read_buf += buf
            if debug:
                if len(buf):
                    #print('RX1: ', binascii.b2a_hex(buf))
                    print('Total: ', len(read_buf))
                    print('Of: ', rxLen)

            if state == RECV_HEAD:
                while len(read_buf):
                    pos = read_buf.find(b'\x04')
                    if pos < 0:
                        read_buf = b''
                        break
                    read_buf = read_buf[pos:]
                    if len(read_buf) >= 7: # minimal len
                        if read_buf[0:2] == b'\x04\x0e' and read_buf[3:6] == b'\x01\xe0\xfc': # maybe a valid rx packet
                            state = RECV_BODY
                            break
                        read_buf = read_buf[1:] # forward to next
                    else:
                        break # get more bytes
            if state == RECV_BODY:
                if len(read_buf) >= rxLen:
                    break
        #read_buf = buf
        #if debug and read_buf:
            #print("RDBUF:", read_buf)
        return read_buf
        # return None

    def LinkCheck(self, stay_in_rom=False):
        if not stay_in_rom:
            txbuf = BuildCmd_LinkCheck()
            rxbuf = self.Start_Cmd(txbuf, CalcRxLength_LinkCheck(), 0.001)
            if rxbuf:
                # print('RX2: ', binascii.b2a_hex(rxbuf, b' '))
                if CheckRespond_LinkCheck(rxbuf):
                    return True
            return False
        else:
            txbuf = BuildCmd_StayRom()
            rxbuf = self.Start_Cmd(txbuf, CalcRxLength_StayRom(), 0.001)
            if rxbuf:
                if CheckRespond_StayRom(rxbuf):
                    return True
            return False

    def SetBR(self, baudrate, delay):
        txbuf = BuildCmd_SetBaudRate(baudrate, delay)
        self.Start_Cmd(txbuf, 0, 0.05)   #
        time.sleep(delay/1000/2)
        self.ser.baudrate = baudrate
        rxbuf = self.WaitForRespond(CalcRxLength_SetBaudRate(), 0.5)
        # print('RX3: ', binascii.b2a_hex(rxbuf, b' '))
        if rxbuf:
            if CheckRespond_SetBaudRate(rxbuf, baudrate, delay):
                return True
        return False

    def ReadSector(self, addr):
        txbuf = BuildCmd_FlashRead4K(addr)
        rxbuf = self.Start_Cmd(txbuf, CalcRxLength_FlashRead4K(), 5)
        if rxbuf:
            ret, _, data = CheckRespond_FlashRead4K(rxbuf, addr)
            if ret:
                return data
        return None

    def EraseBlock(self, val, addr):
        txbuf = BuildCmd_FlashErase(addr, val)
        rxbuf = self.Start_Cmd(txbuf, CalcRxLength_FlashErase(), 1)
        if rxbuf:
            ret, _ = CheckRespond_FlashErase(rxbuf, addr, val)
            if ret:
                return True
        return False

    def WriteSector(self, addr, buf):
        txbuf = BuildCmd_FlashWrite4K(addr, buf)
        rxbuf = self.Start_Cmd(txbuf, CalcRxLength_FlashWrite4K())
        if rxbuf:
            ret, _ = CheckRespond_FlashWrite4K(rxbuf, addr)
            if ret:
                return True
        return False

    def ReadCRC(self, start, end, timeout=5):
        '''
        Read crc of flash region from @start to @end

        return (True_or_False, crc)
        '''
        txbuf = BuildCmd_CheckCRC(start, end)
        rxbuf = self.Start_Cmd(txbuf, CalcRxLength_CheckCRC(), timeout)
        if rxbuf:
            return CheckRespond_CheckCRC(rxbuf, start, end)
        return False,0

    def GetFlashMID(self):
        txbuf = BuildCmd_FlashGetMID(0x9f)
        rxbuf = self.Start_Cmd(txbuf, CalcRxLength_FlashGetID())
        if rxbuf:
            return CheckRespond_FlashGetMID(rxbuf)
        return 0

    def ReadFlashSR(self, regAddr):
        txbuf = BuildCmd_FlashReadSR(regAddr)
        rxbuf = self.Start_Cmd(txbuf, CalcRxLength_FlashReadSR())
        if rxbuf:
            return CheckRespond_FlashReadSR(rxbuf, regAddr)
        return False,0,0

    def WriteFlashSR(self, sz, regAddr, value):
        if sz == 1:
            txbuf = BuildCmd_FlashWriteSR(regAddr, value)
            rxl = CalcRxLength_FlashWriteSR()
        else:
            txbuf = BuildCmd_FlashWriteSR2(regAddr, value)
            rxl = CalcRxLength_FlashWriteSR2()
        rxbuf = self.Start_Cmd(txbuf, rxl)
        if rxbuf:
            if sz == 1:
                return CheckRespond_FlashWriteSR(rxbuf, regAddr, value)
            else:
                return CheckRespond_FlashWriteSR2(rxbuf, regAddr, value)

    def SendReset(self):
        txbuf = BuildCmd_RESET()
        self.Start_Cmd(txbuf)

    def SendReboot(self):
        txbuf = BuildCmd_Reboot()
        self.Start_Cmd(txbuf)

    def SendBkRegReboot(self):
        txbuf = BuildCmd_Bkreg_DoReboot()
        self.Start_Cmd(txbuf)
