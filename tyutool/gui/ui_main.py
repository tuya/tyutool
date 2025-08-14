# -*- coding: utf-8 -*-

################################################################################
## Form generated from reading UI file 'ui_main.ui'
##
## Created by: Qt User Interface Compiler version 6.9.1
##
## WARNING! All changes made in this file will be lost when recompiling UI file!
################################################################################

from PySide6.QtCore import (QCoreApplication, QDate, QDateTime, QLocale,
    QMetaObject, QObject, QPoint, QRect,
    QSize, QTime, QUrl, Qt)
from PySide6.QtGui import (QAction, QBrush, QColor, QConicalGradient,
    QCursor, QFont, QFontDatabase, QGradient,
    QIcon, QImage, QKeySequence, QLinearGradient,
    QPainter, QPalette, QPixmap, QRadialGradient,
    QTransform)
from PySide6.QtWidgets import (QApplication, QCheckBox, QComboBox, QFormLayout,
    QFrame, QGridLayout, QGroupBox, QHBoxLayout,
    QLabel, QLineEdit, QMainWindow, QMenu,
    QMenuBar, QPlainTextEdit, QProgressBar, QPushButton,
    QScrollArea, QSizePolicy, QStatusBar, QTabWidget,
    QTextBrowser, QVBoxLayout, QWidget)

class Ui_MainWindow(object):
    def setupUi(self, MainWindow):
        if not MainWindow.objectName():
            MainWindow.setObjectName(u"MainWindow")
        MainWindow.resize(761, 735)
        self.actionOnDebug = QAction(MainWindow)
        self.actionOnDebug.setObjectName(u"actionOnDebug")
        self.actionOnDebug.setCheckable(False)
        self.actionOnDebug.setEnabled(True)
        self.actionOffDebug = QAction(MainWindow)
        self.actionOffDebug.setObjectName(u"actionOffDebug")
        self.actionOffDebug.setEnabled(False)
        self.actionUpgrade = QAction(MainWindow)
        self.actionUpgrade.setObjectName(u"actionUpgrade")
        self.actionVersion = QAction(MainWindow)
        self.actionVersion.setObjectName(u"actionVersion")
        self.centralwidget = QWidget(MainWindow)
        self.centralwidget.setObjectName(u"centralwidget")
        self.verticalLayout_10 = QVBoxLayout(self.centralwidget)
        self.verticalLayout_10.setObjectName(u"verticalLayout_10")
        self.tabWidget = QTabWidget(self.centralwidget)
        self.tabWidget.setObjectName(u"tabWidget")
        self.tabFlash = QWidget()
        self.tabFlash.setObjectName(u"tabFlash")
        self.verticalLayout_6 = QVBoxLayout(self.tabFlash)
        self.verticalLayout_6.setObjectName(u"verticalLayout_6")
        self.horizontalLayout = QHBoxLayout()
        self.horizontalLayout.setObjectName(u"horizontalLayout")
        self.groupBoxDownloader = QGroupBox(self.tabFlash)
        self.groupBoxDownloader.setObjectName(u"groupBoxDownloader")
        self.verticalLayout_5 = QVBoxLayout(self.groupBoxDownloader)
        self.verticalLayout_5.setObjectName(u"verticalLayout_5")
        self.gridLayout = QGridLayout()
        self.gridLayout.setObjectName(u"gridLayout")
        self.pushButtonStart = QPushButton(self.groupBoxDownloader)
        self.pushButtonStart.setObjectName(u"pushButtonStart")

        self.gridLayout.addWidget(self.pushButtonStart, 6, 1, 1, 1)

        self.labelOperate = QLabel(self.groupBoxDownloader)
        self.labelOperate.setObjectName(u"labelOperate")

        self.gridLayout.addWidget(self.labelOperate, 0, 0, 1, 1)

        self.comboBoxOperate = QComboBox(self.groupBoxDownloader)
        self.comboBoxOperate.setObjectName(u"comboBoxOperate")

        self.gridLayout.addWidget(self.comboBoxOperate, 0, 1, 1, 1)

        self.pushButtonBrowse = QPushButton(self.groupBoxDownloader)
        self.pushButtonBrowse.setObjectName(u"pushButtonBrowse")

        self.gridLayout.addWidget(self.pushButtonBrowse, 1, 2, 1, 1)

        self.labelFile = QLabel(self.groupBoxDownloader)
        self.labelFile.setObjectName(u"labelFile")

        self.gridLayout.addWidget(self.labelFile, 1, 0, 1, 1)

        self.labelBaud = QLabel(self.groupBoxDownloader)
        self.labelBaud.setObjectName(u"labelBaud")

        self.gridLayout.addWidget(self.labelBaud, 3, 0, 1, 1)

        self.labelLength = QLabel(self.groupBoxDownloader)
        self.labelLength.setObjectName(u"labelLength")

        self.gridLayout.addWidget(self.labelLength, 5, 0, 1, 1)

        self.lineEditStart = QLineEdit(self.groupBoxDownloader)
        self.lineEditStart.setObjectName(u"lineEditStart")

        self.gridLayout.addWidget(self.lineEditStart, 4, 1, 1, 1)

        self.comboBoxPort = QComboBox(self.groupBoxDownloader)
        self.comboBoxPort.setObjectName(u"comboBoxPort")

        self.gridLayout.addWidget(self.comboBoxPort, 2, 1, 1, 1)

        self.labelStart = QLabel(self.groupBoxDownloader)
        self.labelStart.setObjectName(u"labelStart")

        self.gridLayout.addWidget(self.labelStart, 4, 0, 1, 1)

        self.lineEditFile = QLineEdit(self.groupBoxDownloader)
        self.lineEditFile.setObjectName(u"lineEditFile")

        self.gridLayout.addWidget(self.lineEditFile, 1, 1, 1, 1)

        self.pushButtonStop = QPushButton(self.groupBoxDownloader)
        self.pushButtonStop.setObjectName(u"pushButtonStop")

        self.gridLayout.addWidget(self.pushButtonStop, 6, 2, 1, 1)

        self.labelPort = QLabel(self.groupBoxDownloader)
        self.labelPort.setObjectName(u"labelPort")

        self.gridLayout.addWidget(self.labelPort, 2, 0, 1, 1)

        self.pushButtonRescan = QPushButton(self.groupBoxDownloader)
        self.pushButtonRescan.setObjectName(u"pushButtonRescan")

        self.gridLayout.addWidget(self.pushButtonRescan, 2, 2, 1, 1)

        self.comboBoxBaud = QComboBox(self.groupBoxDownloader)
        self.comboBoxBaud.setObjectName(u"comboBoxBaud")
        self.comboBoxBaud.setEditable(True)

        self.gridLayout.addWidget(self.comboBoxBaud, 3, 1, 1, 1)

        self.lineEditLength = QLineEdit(self.groupBoxDownloader)
        self.lineEditLength.setObjectName(u"lineEditLength")
        self.lineEditLength.setEnabled(True)

        self.gridLayout.addWidget(self.lineEditLength, 5, 1, 1, 1)


        self.verticalLayout_5.addLayout(self.gridLayout)


        self.horizontalLayout.addWidget(self.groupBoxDownloader)

        self.verticalLayout = QVBoxLayout()
        self.verticalLayout.setObjectName(u"verticalLayout")
        self.groupBoxChipView = QGroupBox(self.tabFlash)
        self.groupBoxChipView.setObjectName(u"groupBoxChipView")
        self.verticalLayout_3 = QVBoxLayout(self.groupBoxChipView)
        self.verticalLayout_3.setObjectName(u"verticalLayout_3")
        self.horizontalLayout_5 = QHBoxLayout()
        self.horizontalLayout_5.setObjectName(u"horizontalLayout_5")
        self.labelChip = QLabel(self.groupBoxChipView)
        self.labelChip.setObjectName(u"labelChip")

        self.horizontalLayout_5.addWidget(self.labelChip)

        self.comboBoxChip = QComboBox(self.groupBoxChipView)
        self.comboBoxChip.setObjectName(u"comboBoxChip")

        self.horizontalLayout_5.addWidget(self.comboBoxChip)


        self.verticalLayout_3.addLayout(self.horizontalLayout_5)

        self.horizontalLayout_6 = QHBoxLayout()
        self.horizontalLayout_6.setObjectName(u"horizontalLayout_6")
        self.labelModule = QLabel(self.groupBoxChipView)
        self.labelModule.setObjectName(u"labelModule")

        self.horizontalLayout_6.addWidget(self.labelModule)

        self.comboBoxModule = QComboBox(self.groupBoxChipView)
        self.comboBoxModule.setObjectName(u"comboBoxModule")

        self.horizontalLayout_6.addWidget(self.comboBoxModule)


        self.verticalLayout_3.addLayout(self.horizontalLayout_6)

        self.verticalLayout_4 = QVBoxLayout()
        self.verticalLayout_4.setObjectName(u"verticalLayout_4")
        self.labelModuleUrl = QLabel(self.groupBoxChipView)
        self.labelModuleUrl.setObjectName(u"labelModuleUrl")
        font = QFont()
        font.setUnderline(True)
        self.labelModuleUrl.setFont(font)
        self.labelModuleUrl.setAlignment(Qt.AlignmentFlag.AlignRight|Qt.AlignmentFlag.AlignTrailing|Qt.AlignmentFlag.AlignVCenter)
        self.labelModuleUrl.setOpenExternalLinks(True)

        self.verticalLayout_4.addWidget(self.labelModuleUrl)


        self.verticalLayout_3.addLayout(self.verticalLayout_4)


        self.verticalLayout.addWidget(self.groupBoxChipView)

        self.labelModulePic = QLabel(self.tabFlash)
        self.labelModulePic.setObjectName(u"labelModulePic")
        sizePolicy = QSizePolicy(QSizePolicy.Policy.Preferred, QSizePolicy.Policy.Preferred)
        sizePolicy.setHorizontalStretch(0)
        sizePolicy.setVerticalStretch(0)
        sizePolicy.setHeightForWidth(self.labelModulePic.sizePolicy().hasHeightForWidth())
        self.labelModulePic.setSizePolicy(sizePolicy)
        self.labelModulePic.setMaximumSize(QSize(200, 200))
        self.labelModulePic.setCursor(QCursor(Qt.CursorShape.ForbiddenCursor))
        self.labelModulePic.setFrameShape(QFrame.Shape.StyledPanel)
        self.labelModulePic.setTextInteractionFlags(Qt.TextInteractionFlag.LinksAccessibleByMouse)

        self.verticalLayout.addWidget(self.labelModulePic)


        self.horizontalLayout.addLayout(self.verticalLayout)


        self.verticalLayout_6.addLayout(self.horizontalLayout)

        self.progressBarShow = QProgressBar(self.tabFlash)
        self.progressBarShow.setObjectName(u"progressBarShow")
        self.progressBarShow.setMaximum(10)
        self.progressBarShow.setValue(0)

        self.verticalLayout_6.addWidget(self.progressBarShow)

        self.textBrowserShow = QTextBrowser(self.tabFlash)
        self.textBrowserShow.setObjectName(u"textBrowserShow")

        self.verticalLayout_6.addWidget(self.textBrowserShow)

        self.tabWidget.addTab(self.tabFlash, "")
        self.tabSerial = QWidget()
        self.tabSerial.setObjectName(u"tabSerial")
        self.verticalLayout_2 = QVBoxLayout(self.tabSerial)
        self.verticalLayout_2.setObjectName(u"verticalLayout_2")
        self.groupBoxRx = QGroupBox(self.tabSerial)
        self.groupBoxRx.setObjectName(u"groupBoxRx")
        self.horizontalLayout_7 = QHBoxLayout(self.groupBoxRx)
        self.horizontalLayout_7.setObjectName(u"horizontalLayout_7")
        self.verticalLayout_8 = QVBoxLayout()
        self.verticalLayout_8.setObjectName(u"verticalLayout_8")
        self.formLayout_2 = QFormLayout()
        self.formLayout_2.setObjectName(u"formLayout_2")
        self.pushButtonCom = QPushButton(self.groupBoxRx)
        self.pushButtonCom.setObjectName(u"pushButtonCom")

        self.formLayout_2.setWidget(0, QFormLayout.ItemRole.LabelRole, self.pushButtonCom)

        self.comboBoxCom = QComboBox(self.groupBoxRx)
        self.comboBoxCom.setObjectName(u"comboBoxCom")

        self.formLayout_2.setWidget(0, QFormLayout.ItemRole.FieldRole, self.comboBoxCom)

        self.labelBaudRate = QLabel(self.groupBoxRx)
        self.labelBaudRate.setObjectName(u"labelBaudRate")

        self.formLayout_2.setWidget(1, QFormLayout.ItemRole.LabelRole, self.labelBaudRate)

        self.comboBoxSBaud = QComboBox(self.groupBoxRx)
        self.comboBoxSBaud.setObjectName(u"comboBoxSBaud")
        self.comboBoxSBaud.setEditable(True)

        self.formLayout_2.setWidget(1, QFormLayout.ItemRole.FieldRole, self.comboBoxSBaud)

        self.labelDataBits = QLabel(self.groupBoxRx)
        self.labelDataBits.setObjectName(u"labelDataBits")

        self.formLayout_2.setWidget(2, QFormLayout.ItemRole.LabelRole, self.labelDataBits)

        self.comboBoxDataBits = QComboBox(self.groupBoxRx)
        self.comboBoxDataBits.setObjectName(u"comboBoxDataBits")

        self.formLayout_2.setWidget(2, QFormLayout.ItemRole.FieldRole, self.comboBoxDataBits)

        self.labelParity = QLabel(self.groupBoxRx)
        self.labelParity.setObjectName(u"labelParity")

        self.formLayout_2.setWidget(3, QFormLayout.ItemRole.LabelRole, self.labelParity)

        self.comboBoxParity = QComboBox(self.groupBoxRx)
        self.comboBoxParity.setObjectName(u"comboBoxParity")

        self.formLayout_2.setWidget(3, QFormLayout.ItemRole.FieldRole, self.comboBoxParity)

        self.labelStopBits = QLabel(self.groupBoxRx)
        self.labelStopBits.setObjectName(u"labelStopBits")

        self.formLayout_2.setWidget(4, QFormLayout.ItemRole.LabelRole, self.labelStopBits)

        self.comboBoxStopBits = QComboBox(self.groupBoxRx)
        self.comboBoxStopBits.setObjectName(u"comboBoxStopBits")

        self.formLayout_2.setWidget(4, QFormLayout.ItemRole.FieldRole, self.comboBoxStopBits)


        self.verticalLayout_8.addLayout(self.formLayout_2)

        self.verticalLayout_7 = QVBoxLayout()
        self.verticalLayout_7.setObjectName(u"verticalLayout_7")
        self.pushButtonSStart = QPushButton(self.groupBoxRx)
        self.pushButtonSStart.setObjectName(u"pushButtonSStart")

        self.verticalLayout_7.addWidget(self.pushButtonSStart)

        self.horizontalLayout_4 = QHBoxLayout()
        self.horizontalLayout_4.setObjectName(u"horizontalLayout_4")
        self.pushButtonClear = QPushButton(self.groupBoxRx)
        self.pushButtonClear.setObjectName(u"pushButtonClear")

        self.horizontalLayout_4.addWidget(self.pushButtonClear)

        self.pushButtonSave = QPushButton(self.groupBoxRx)
        self.pushButtonSave.setObjectName(u"pushButtonSave")

        self.horizontalLayout_4.addWidget(self.pushButtonSave)


        self.verticalLayout_7.addLayout(self.horizontalLayout_4)

        self.horizontalLayout_2 = QHBoxLayout()
        self.horizontalLayout_2.setObjectName(u"horizontalLayout_2")
        self.checkBoxRxTime = QCheckBox(self.groupBoxRx)
        self.checkBoxRxTime.setObjectName(u"checkBoxRxTime")

        self.horizontalLayout_2.addWidget(self.checkBoxRxTime)

        self.checkBoxRxHex = QCheckBox(self.groupBoxRx)
        self.checkBoxRxHex.setObjectName(u"checkBoxRxHex")

        self.horizontalLayout_2.addWidget(self.checkBoxRxHex)


        self.verticalLayout_7.addLayout(self.horizontalLayout_2)


        self.verticalLayout_8.addLayout(self.verticalLayout_7)

        self.pushButtonAuth = QPushButton(self.groupBoxRx)
        self.pushButtonAuth.setObjectName(u"pushButtonAuth")

        self.verticalLayout_8.addWidget(self.pushButtonAuth)


        self.horizontalLayout_7.addLayout(self.verticalLayout_8)

        self.textBrowserRx = QTextBrowser(self.groupBoxRx)
        self.textBrowserRx.setObjectName(u"textBrowserRx")

        self.horizontalLayout_7.addWidget(self.textBrowserRx)


        self.verticalLayout_2.addWidget(self.groupBoxRx)

        self.groupBoxTx = QGroupBox(self.tabSerial)
        self.groupBoxTx.setObjectName(u"groupBoxTx")
        self.verticalLayout_9 = QVBoxLayout(self.groupBoxTx)
        self.verticalLayout_9.setObjectName(u"verticalLayout_9")
        self.horizontalLayout_10 = QHBoxLayout()
        self.horizontalLayout_10.setObjectName(u"horizontalLayout_10")
        self.horizontalLayout_3 = QHBoxLayout()
        self.horizontalLayout_3.setObjectName(u"horizontalLayout_3")
        self.pushButtonSend = QPushButton(self.groupBoxTx)
        self.pushButtonSend.setObjectName(u"pushButtonSend")

        self.horizontalLayout_3.addWidget(self.pushButtonSend)

        self.checkBoxTxHex = QCheckBox(self.groupBoxTx)
        self.checkBoxTxHex.setObjectName(u"checkBoxTxHex")

        self.horizontalLayout_3.addWidget(self.checkBoxTxHex)

        self.checkBoxTxTime = QCheckBox(self.groupBoxTx)
        self.checkBoxTxTime.setObjectName(u"checkBoxTxTime")

        self.horizontalLayout_3.addWidget(self.checkBoxTxTime)

        self.checkBoxTxReturn = QCheckBox(self.groupBoxTx)
        self.checkBoxTxReturn.setObjectName(u"checkBoxTxReturn")
        self.checkBoxTxReturn.setChecked(True)

        self.horizontalLayout_3.addWidget(self.checkBoxTxReturn)


        self.horizontalLayout_10.addLayout(self.horizontalLayout_3)


        self.verticalLayout_9.addLayout(self.horizontalLayout_10)

        self.plainTextEditTx = QPlainTextEdit(self.groupBoxTx)
        self.plainTextEditTx.setObjectName(u"plainTextEditTx")
        sizePolicy1 = QSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        sizePolicy1.setHorizontalStretch(0)
        sizePolicy1.setVerticalStretch(0)
        sizePolicy1.setHeightForWidth(self.plainTextEditTx.sizePolicy().hasHeightForWidth())
        self.plainTextEditTx.setSizePolicy(sizePolicy1)
        self.plainTextEditTx.setMaximumSize(QSize(16777215, 100))

        self.verticalLayout_9.addWidget(self.plainTextEditTx)


        self.verticalLayout_2.addWidget(self.groupBoxTx)

        self.tabWidget.addTab(self.tabSerial, "")
        self.tabSerDebug = QWidget()
        self.tabSerDebug.setObjectName(u"tabSerDebug")
        self.verticalLayout_12 = QVBoxLayout(self.tabSerDebug)
        self.verticalLayout_12.setObjectName(u"verticalLayout_12")
        self.horizontalLayout_8 = QHBoxLayout()
        self.horizontalLayout_8.setObjectName(u"horizontalLayout_8")
        self.horizontalLayout_29 = QHBoxLayout()
        self.horizontalLayout_29.setObjectName(u"horizontalLayout_29")
        self.pushButtonSDCom = QPushButton(self.tabSerDebug)
        self.pushButtonSDCom.setObjectName(u"pushButtonSDCom")
        sizePolicy2 = QSizePolicy(QSizePolicy.Policy.Fixed, QSizePolicy.Policy.Fixed)
        sizePolicy2.setHorizontalStretch(0)
        sizePolicy2.setVerticalStretch(0)
        sizePolicy2.setHeightForWidth(self.pushButtonSDCom.sizePolicy().hasHeightForWidth())
        self.pushButtonSDCom.setSizePolicy(sizePolicy2)

        self.horizontalLayout_29.addWidget(self.pushButtonSDCom)

        self.comboBoxSDPort = QComboBox(self.tabSerDebug)
        self.comboBoxSDPort.setObjectName(u"comboBoxSDPort")

        self.horizontalLayout_29.addWidget(self.comboBoxSDPort)


        self.horizontalLayout_8.addLayout(self.horizontalLayout_29)

        self.horizontalLayout_30 = QHBoxLayout()
        self.horizontalLayout_30.setObjectName(u"horizontalLayout_30")
        self.labelSDBaud = QLabel(self.tabSerDebug)
        self.labelSDBaud.setObjectName(u"labelSDBaud")
        sizePolicy3 = QSizePolicy(QSizePolicy.Policy.Fixed, QSizePolicy.Policy.Preferred)
        sizePolicy3.setHorizontalStretch(0)
        sizePolicy3.setVerticalStretch(0)
        sizePolicy3.setHeightForWidth(self.labelSDBaud.sizePolicy().hasHeightForWidth())
        self.labelSDBaud.setSizePolicy(sizePolicy3)

        self.horizontalLayout_30.addWidget(self.labelSDBaud)

        self.comboBoxSDBaud = QComboBox(self.tabSerDebug)
        self.comboBoxSDBaud.setObjectName(u"comboBoxSDBaud")
        self.comboBoxSDBaud.setEditable(True)

        self.horizontalLayout_30.addWidget(self.comboBoxSDBaud)


        self.horizontalLayout_8.addLayout(self.horizontalLayout_30)

        self.pushButtonSDConnect = QPushButton(self.tabSerDebug)
        self.pushButtonSDConnect.setObjectName(u"pushButtonSDConnect")

        self.horizontalLayout_8.addWidget(self.pushButtonSDConnect)


        self.verticalLayout_12.addLayout(self.horizontalLayout_8)

        self.horizontalLayout_9 = QHBoxLayout()
        self.horizontalLayout_9.setObjectName(u"horizontalLayout_9")
        self.scrollArea = QScrollArea(self.tabSerDebug)
        self.scrollArea.setObjectName(u"scrollArea")
        sizePolicy4 = QSizePolicy(QSizePolicy.Policy.Maximum, QSizePolicy.Policy.Expanding)
        sizePolicy4.setHorizontalStretch(0)
        sizePolicy4.setVerticalStretch(0)
        sizePolicy4.setHeightForWidth(self.scrollArea.sizePolicy().hasHeightForWidth())
        self.scrollArea.setSizePolicy(sizePolicy4)
        self.scrollArea.setMaximumSize(QSize(320, 16777215))
        self.scrollArea.setWidgetResizable(True)
        self.scrollAreaWidgetContents = QWidget()
        self.scrollAreaWidgetContents.setObjectName(u"scrollAreaWidgetContents")
        self.scrollAreaWidgetContents.setGeometry(QRect(0, -147, 297, 728))
        self.verticalLayout_11 = QVBoxLayout(self.scrollAreaWidgetContents)
        self.verticalLayout_11.setSpacing(10)
        self.verticalLayout_11.setObjectName(u"verticalLayout_11")
        self.horizontalLayout_12 = QHBoxLayout()
        self.horizontalLayout_12.setObjectName(u"horizontalLayout_12")
        self.pushButtonSDStart = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDStart.setObjectName(u"pushButtonSDStart")
        self.pushButtonSDStart.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_12.addWidget(self.pushButtonSDStart)


        self.verticalLayout_11.addLayout(self.horizontalLayout_12)

        self.horizontalLayout_13 = QHBoxLayout()
        self.horizontalLayout_13.setObjectName(u"horizontalLayout_13")
        self.pushButtonSDStop = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDStop.setObjectName(u"pushButtonSDStop")
        self.pushButtonSDStop.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_13.addWidget(self.pushButtonSDStop)


        self.verticalLayout_11.addLayout(self.horizontalLayout_13)

        self.horizontalLayout_14 = QHBoxLayout()
        self.horizontalLayout_14.setObjectName(u"horizontalLayout_14")
        self.pushButtonSDReset = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDReset.setObjectName(u"pushButtonSDReset")
        self.pushButtonSDReset.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_14.addWidget(self.pushButtonSDReset)


        self.verticalLayout_11.addLayout(self.horizontalLayout_14)

        self.horizontalLayout_15 = QHBoxLayout()
        self.horizontalLayout_15.setObjectName(u"horizontalLayout_15")
        self.pushButtonSDDump0 = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDDump0.setObjectName(u"pushButtonSDDump0")
        self.pushButtonSDDump0.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_15.addWidget(self.pushButtonSDDump0)


        self.verticalLayout_11.addLayout(self.horizontalLayout_15)

        self.horizontalLayout_16 = QHBoxLayout()
        self.horizontalLayout_16.setObjectName(u"horizontalLayout_16")
        self.pushButtonSDDump1 = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDDump1.setObjectName(u"pushButtonSDDump1")
        self.pushButtonSDDump1.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_16.addWidget(self.pushButtonSDDump1)


        self.verticalLayout_11.addLayout(self.horizontalLayout_16)

        self.horizontalLayout_17 = QHBoxLayout()
        self.horizontalLayout_17.setObjectName(u"horizontalLayout_17")
        self.pushButtonSDDump2 = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDDump2.setObjectName(u"pushButtonSDDump2")
        self.pushButtonSDDump2.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_17.addWidget(self.pushButtonSDDump2)


        self.verticalLayout_11.addLayout(self.horizontalLayout_17)

        self.horizontalLayout_18 = QHBoxLayout()
        self.horizontalLayout_18.setObjectName(u"horizontalLayout_18")
        self.pushButtonSDBg0 = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDBg0.setObjectName(u"pushButtonSDBg0")
        self.pushButtonSDBg0.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_18.addWidget(self.pushButtonSDBg0)


        self.verticalLayout_11.addLayout(self.horizontalLayout_18)

        self.verticalLayout_13 = QVBoxLayout()
        self.verticalLayout_13.setObjectName(u"verticalLayout_13")
        self.pushButtonSDBg1 = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDBg1.setObjectName(u"pushButtonSDBg1")
        self.pushButtonSDBg1.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.verticalLayout_13.addWidget(self.pushButtonSDBg1)


        self.verticalLayout_11.addLayout(self.verticalLayout_13)

        self.horizontalLayout_20 = QHBoxLayout()
        self.horizontalLayout_20.setObjectName(u"horizontalLayout_20")
        self.pushButtonSDBg2 = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDBg2.setObjectName(u"pushButtonSDBg2")
        self.pushButtonSDBg2.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_20.addWidget(self.pushButtonSDBg2)


        self.verticalLayout_11.addLayout(self.horizontalLayout_20)

        self.horizontalLayout_21 = QHBoxLayout()
        self.horizontalLayout_21.setObjectName(u"horizontalLayout_21")
        self.pushButtonSDBg3 = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDBg3.setObjectName(u"pushButtonSDBg3")
        self.pushButtonSDBg3.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_21.addWidget(self.pushButtonSDBg3)


        self.verticalLayout_11.addLayout(self.horizontalLayout_21)

        self.horizontalLayout_22 = QHBoxLayout()
        self.horizontalLayout_22.setObjectName(u"horizontalLayout_22")
        self.pushButtonSDBg4 = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDBg4.setObjectName(u"pushButtonSDBg4")
        self.pushButtonSDBg4.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_22.addWidget(self.pushButtonSDBg4)


        self.verticalLayout_11.addLayout(self.horizontalLayout_22)

        self.horizontalLayout_23 = QHBoxLayout()
        self.horizontalLayout_23.setObjectName(u"horizontalLayout_23")
        self.pushButtonSDVolume = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDVolume.setObjectName(u"pushButtonSDVolume")
        self.pushButtonSDVolume.setMinimumSize(QSize(120, 0))
        self.pushButtonSDVolume.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_23.addWidget(self.pushButtonSDVolume)

        self.lineEditSDVolume = QLineEdit(self.scrollAreaWidgetContents)
        self.lineEditSDVolume.setObjectName(u"lineEditSDVolume")

        self.horizontalLayout_23.addWidget(self.lineEditSDVolume)

        self.label_3 = QLabel(self.scrollAreaWidgetContents)
        self.label_3.setObjectName(u"label_3")

        self.horizontalLayout_23.addWidget(self.label_3)


        self.verticalLayout_11.addLayout(self.horizontalLayout_23)

        self.horizontalLayout_11 = QHBoxLayout()
        self.horizontalLayout_11.setObjectName(u"horizontalLayout_11")
        self.pushButtonSDMicgain = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDMicgain.setObjectName(u"pushButtonSDMicgain")
        self.pushButtonSDMicgain.setMinimumSize(QSize(120, 0))
        self.pushButtonSDMicgain.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_11.addWidget(self.pushButtonSDMicgain)

        self.lineEditSDMicgain = QLineEdit(self.scrollAreaWidgetContents)
        self.lineEditSDMicgain.setObjectName(u"lineEditSDMicgain")

        self.horizontalLayout_11.addWidget(self.lineEditSDMicgain)


        self.verticalLayout_11.addLayout(self.horizontalLayout_11)

        self.horizontalLayout_24 = QHBoxLayout()
        self.horizontalLayout_24.setObjectName(u"horizontalLayout_24")
        self.pushButtonSDAlgSet = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDAlgSet.setObjectName(u"pushButtonSDAlgSet")
        self.pushButtonSDAlgSet.setMinimumSize(QSize(65, 0))
        self.pushButtonSDAlgSet.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_24.addWidget(self.pushButtonSDAlgSet)

        self.lineEditSDAlgSetP = QLineEdit(self.scrollAreaWidgetContents)
        self.lineEditSDAlgSetP.setObjectName(u"lineEditSDAlgSetP")
        sizePolicy5 = QSizePolicy(QSizePolicy.Policy.Minimum, QSizePolicy.Policy.Fixed)
        sizePolicy5.setHorizontalStretch(0)
        sizePolicy5.setVerticalStretch(0)
        sizePolicy5.setHeightForWidth(self.lineEditSDAlgSetP.sizePolicy().hasHeightForWidth())
        self.lineEditSDAlgSetP.setSizePolicy(sizePolicy5)
        self.lineEditSDAlgSetP.setMinimumSize(QSize(140, 0))

        self.horizontalLayout_24.addWidget(self.lineEditSDAlgSetP)

        self.lineEditSDAlgSetV = QLineEdit(self.scrollAreaWidgetContents)
        self.lineEditSDAlgSetV.setObjectName(u"lineEditSDAlgSetV")
        sizePolicy5.setHeightForWidth(self.lineEditSDAlgSetV.sizePolicy().hasHeightForWidth())
        self.lineEditSDAlgSetV.setSizePolicy(sizePolicy5)
        self.lineEditSDAlgSetV.setMinimumSize(QSize(50, 0))

        self.horizontalLayout_24.addWidget(self.lineEditSDAlgSetV)


        self.verticalLayout_11.addLayout(self.horizontalLayout_24)

        self.horizontalLayout_25 = QHBoxLayout()
        self.horizontalLayout_25.setObjectName(u"horizontalLayout_25")
        self.pushButtonSDAlgSetVad = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDAlgSetVad.setObjectName(u"pushButtonSDAlgSetVad")
        self.pushButtonSDAlgSetVad.setMinimumSize(QSize(120, 0))
        self.pushButtonSDAlgSetVad.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_25.addWidget(self.pushButtonSDAlgSetVad)

        self.lineEditSDAlgSetVadC = QLineEdit(self.scrollAreaWidgetContents)
        self.lineEditSDAlgSetVadC.setObjectName(u"lineEditSDAlgSetVadC")

        self.horizontalLayout_25.addWidget(self.lineEditSDAlgSetVadC)

        self.lineEditSDAlgSetVadV = QLineEdit(self.scrollAreaWidgetContents)
        self.lineEditSDAlgSetVadV.setObjectName(u"lineEditSDAlgSetVadV")

        self.horizontalLayout_25.addWidget(self.lineEditSDAlgSetVadV)


        self.verticalLayout_11.addLayout(self.horizontalLayout_25)

        self.horizontalLayout_26 = QHBoxLayout()
        self.horizontalLayout_26.setObjectName(u"horizontalLayout_26")
        self.pushButtonSDAlgGet = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDAlgGet.setObjectName(u"pushButtonSDAlgGet")
        self.pushButtonSDAlgGet.setMinimumSize(QSize(65, 0))
        self.pushButtonSDAlgGet.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_26.addWidget(self.pushButtonSDAlgGet)

        self.lineEditSDAlgGetP = QLineEdit(self.scrollAreaWidgetContents)
        self.lineEditSDAlgGetP.setObjectName(u"lineEditSDAlgGetP")

        self.horizontalLayout_26.addWidget(self.lineEditSDAlgGetP)


        self.verticalLayout_11.addLayout(self.horizontalLayout_26)

        self.horizontalLayout_28 = QHBoxLayout()
        self.horizontalLayout_28.setObjectName(u"horizontalLayout_28")
        self.pushButtonSDAlgDump = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonSDAlgDump.setObjectName(u"pushButtonSDAlgDump")
        self.pushButtonSDAlgDump.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.horizontalLayout_28.addWidget(self.pushButtonSDAlgDump)


        self.verticalLayout_11.addLayout(self.horizontalLayout_28)

        self.verticalLayout_14 = QVBoxLayout()
        self.verticalLayout_14.setObjectName(u"verticalLayout_14")
        self.pushButtonAutoTest = QPushButton(self.scrollAreaWidgetContents)
        self.pushButtonAutoTest.setObjectName(u"pushButtonAutoTest")
        self.pushButtonAutoTest.setStyleSheet(u"text-align: left;\n"
"padding-left: 10px;\n"
"padding-top: 4px;\n"
"padding-bottom: 4px;")

        self.verticalLayout_14.addWidget(self.pushButtonAutoTest)


        self.verticalLayout_11.addLayout(self.verticalLayout_14)

        self.scrollArea.setWidget(self.scrollAreaWidgetContents)

        self.horizontalLayout_9.addWidget(self.scrollArea)

        self.textBrowserSD = QTextBrowser(self.tabSerDebug)
        self.textBrowserSD.setObjectName(u"textBrowserSD")
        sizePolicy6 = QSizePolicy(QSizePolicy.Policy.Minimum, QSizePolicy.Policy.Expanding)
        sizePolicy6.setHorizontalStretch(0)
        sizePolicy6.setVerticalStretch(0)
        sizePolicy6.setHeightForWidth(self.textBrowserSD.sizePolicy().hasHeightForWidth())
        self.textBrowserSD.setSizePolicy(sizePolicy6)
        self.textBrowserSD.setMinimumSize(QSize(400, 0))

        self.horizontalLayout_9.addWidget(self.textBrowserSD)


        self.verticalLayout_12.addLayout(self.horizontalLayout_9)

        self.tabWidget.addTab(self.tabSerDebug, "")

        self.verticalLayout_10.addWidget(self.tabWidget)

        MainWindow.setCentralWidget(self.centralwidget)
        self.menubar = QMenuBar(MainWindow)
        self.menubar.setObjectName(u"menubar")
        self.menubar.setGeometry(QRect(0, 0, 761, 23))
        self.menuMain = QMenu(self.menubar)
        self.menuMain.setObjectName(u"menuMain")
        self.menuDebug = QMenu(self.menuMain)
        self.menuDebug.setObjectName(u"menuDebug")
        MainWindow.setMenuBar(self.menubar)
        self.statusbar = QStatusBar(MainWindow)
        self.statusbar.setObjectName(u"statusbar")
        MainWindow.setStatusBar(self.statusbar)

        self.menubar.addAction(self.menuMain.menuAction())
        self.menuMain.addAction(self.menuDebug.menuAction())
        self.menuMain.addSeparator()
        self.menuMain.addAction(self.actionVersion)
        self.menuMain.addAction(self.actionUpgrade)
        self.menuDebug.addAction(self.actionOnDebug)
        self.menuDebug.addAction(self.actionOffDebug)

        self.retranslateUi(MainWindow)

        self.tabWidget.setCurrentIndex(0)


        QMetaObject.connectSlotsByName(MainWindow)
    # setupUi

    def retranslateUi(self, MainWindow):
        MainWindow.setWindowTitle(QCoreApplication.translate("MainWindow", u"Tuya Uart Tool", None))
        self.actionOnDebug.setText(QCoreApplication.translate("MainWindow", u"On", None))
        self.actionOffDebug.setText(QCoreApplication.translate("MainWindow", u"Off", None))
        self.actionUpgrade.setText(QCoreApplication.translate("MainWindow", u"Upgrade", None))
        self.actionVersion.setText(QCoreApplication.translate("MainWindow", u"Version", None))
        self.groupBoxDownloader.setTitle(QCoreApplication.translate("MainWindow", u"Downloader", None))
        self.pushButtonStart.setText(QCoreApplication.translate("MainWindow", u"Start", None))
        self.labelOperate.setText(QCoreApplication.translate("MainWindow", u"Operate", None))
        self.pushButtonBrowse.setText(QCoreApplication.translate("MainWindow", u"Browse", None))
        self.labelFile.setText(QCoreApplication.translate("MainWindow", u"File", None))
        self.labelBaud.setText(QCoreApplication.translate("MainWindow", u"BaudRate", None))
        self.labelLength.setText(QCoreApplication.translate("MainWindow", u"Length: ", None))
        self.lineEditStart.setText("")
        self.labelStart.setText(QCoreApplication.translate("MainWindow", u"Start: ", None))
        self.pushButtonStop.setText(QCoreApplication.translate("MainWindow", u"Stop", None))
        self.labelPort.setText(QCoreApplication.translate("MainWindow", u"PortNum", None))
        self.pushButtonRescan.setText(QCoreApplication.translate("MainWindow", u"Rescan", None))
        self.comboBoxBaud.setCurrentText("")
        self.lineEditLength.setText("")
        self.groupBoxChipView.setTitle(QCoreApplication.translate("MainWindow", u"ChipView", None))
        self.labelChip.setText(QCoreApplication.translate("MainWindow", u"Chip", None))
        self.labelModule.setText(QCoreApplication.translate("MainWindow", u"Module", None))
        self.labelModuleUrl.setText(QCoreApplication.translate("MainWindow", u"<a href=\"https://iot.tuya.com\">Tuya</a>", None))
        self.labelModulePic.setText("")
        self.tabWidget.setTabText(self.tabWidget.indexOf(self.tabFlash), QCoreApplication.translate("MainWindow", u"Flash", None))
        self.groupBoxRx.setTitle(QCoreApplication.translate("MainWindow", u"RX", None))
        self.pushButtonCom.setText(QCoreApplication.translate("MainWindow", u"PortScan", None))
        self.labelBaudRate.setText(QCoreApplication.translate("MainWindow", u"BaudRate", None))
        self.labelDataBits.setText(QCoreApplication.translate("MainWindow", u"DataBits", None))
        self.labelParity.setText(QCoreApplication.translate("MainWindow", u"Parity", None))
        self.labelStopBits.setText(QCoreApplication.translate("MainWindow", u"StopBits", None))
        self.pushButtonSStart.setText(QCoreApplication.translate("MainWindow", u"Start", None))
        self.pushButtonClear.setText(QCoreApplication.translate("MainWindow", u"Clear", None))
        self.pushButtonSave.setText(QCoreApplication.translate("MainWindow", u"Save", None))
        self.checkBoxRxTime.setText(QCoreApplication.translate("MainWindow", u"Time", None))
        self.checkBoxRxHex.setText(QCoreApplication.translate("MainWindow", u"Hex", None))
        self.pushButtonAuth.setText(QCoreApplication.translate("MainWindow", u"Authorize", None))
        self.groupBoxTx.setTitle(QCoreApplication.translate("MainWindow", u"TX", None))
        self.pushButtonSend.setText(QCoreApplication.translate("MainWindow", u"Send", None))
        self.checkBoxTxHex.setText(QCoreApplication.translate("MainWindow", u"Hex", None))
        self.checkBoxTxTime.setText(QCoreApplication.translate("MainWindow", u"Time", None))
        self.checkBoxTxReturn.setText(QCoreApplication.translate("MainWindow", u"\"\\n\"", None))
        self.tabWidget.setTabText(self.tabWidget.indexOf(self.tabSerial), QCoreApplication.translate("MainWindow", u"Serial", None))
        self.pushButtonSDCom.setText(QCoreApplication.translate("MainWindow", u"PortScan:", None))
        self.labelSDBaud.setText(QCoreApplication.translate("MainWindow", u"BaudRate:", None))
        self.pushButtonSDConnect.setText(QCoreApplication.translate("MainWindow", u"Connect", None))
        self.pushButtonSDStart.setText(QCoreApplication.translate("MainWindow", u"start recording", None))
        self.pushButtonSDStop.setText(QCoreApplication.translate("MainWindow", u"stop recording", None))
        self.pushButtonSDReset.setText(QCoreApplication.translate("MainWindow", u"reset recording", None))
        self.pushButtonSDDump0.setText(QCoreApplication.translate("MainWindow", u"dump: microphone channel", None))
        self.pushButtonSDDump1.setText(QCoreApplication.translate("MainWindow", u"dump: reference channel", None))
        self.pushButtonSDDump2.setText(QCoreApplication.translate("MainWindow", u"dump: AEC channel", None))
        self.pushButtonSDBg0.setText(QCoreApplication.translate("MainWindow", u"play: white noise", None))
        self.pushButtonSDBg1.setText(QCoreApplication.translate("MainWindow", u"play: 1K-0dB", None))
        self.pushButtonSDBg2.setText(QCoreApplication.translate("MainWindow", u"play: sweep frequency constantly", None))
        self.pushButtonSDBg3.setText(QCoreApplication.translate("MainWindow", u"play: sweep discrete frequency", None))
        self.pushButtonSDBg4.setText(QCoreApplication.translate("MainWindow", u"play: min single frequency", None))
        self.pushButtonSDVolume.setText(QCoreApplication.translate("MainWindow", u"set volume", None))
        self.lineEditSDVolume.setInputMask("")
        self.lineEditSDVolume.setPlaceholderText(QCoreApplication.translate("MainWindow", u"70", None))
        self.label_3.setText(QCoreApplication.translate("MainWindow", u"%", None))
        self.pushButtonSDMicgain.setText(QCoreApplication.translate("MainWindow", u"set micgain", None))
        self.lineEditSDMicgain.setPlaceholderText(QCoreApplication.translate("MainWindow", u"70", None))
        self.pushButtonSDAlgSet.setText(QCoreApplication.translate("MainWindow", u"alg set", None))
        self.lineEditSDAlgSetP.setPlaceholderText(QCoreApplication.translate("MainWindow", u"aec_ec_depth", None))
        self.lineEditSDAlgSetV.setPlaceholderText(QCoreApplication.translate("MainWindow", u"1", None))
        self.pushButtonSDAlgSetVad.setText(QCoreApplication.translate("MainWindow", u"set vad_SPthr", None))
        self.lineEditSDAlgSetVadC.setPlaceholderText(QCoreApplication.translate("MainWindow", u"0", None))
        self.lineEditSDAlgSetVadV.setPlaceholderText(QCoreApplication.translate("MainWindow", u"1000", None))
        self.pushButtonSDAlgGet.setText(QCoreApplication.translate("MainWindow", u"alg get", None))
        self.lineEditSDAlgGetP.setPlaceholderText(QCoreApplication.translate("MainWindow", u"aec_ec_depth", None))
        self.pushButtonSDAlgDump.setText(QCoreApplication.translate("MainWindow", u"alg dump", None))
        self.pushButtonAutoTest.setText(QCoreApplication.translate("MainWindow", u"Auto test and gen reports", None))
        self.tabWidget.setTabText(self.tabWidget.indexOf(self.tabSerDebug), QCoreApplication.translate("MainWindow", u"SerDebug", None))
        self.menuMain.setTitle(QCoreApplication.translate("MainWindow", u"Main", None))
        self.menuDebug.setTitle(QCoreApplication.translate("MainWindow", u"Debug", None))
    # retranslateUi

