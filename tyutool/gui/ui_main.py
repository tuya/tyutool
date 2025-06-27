# -*- coding: utf-8 -*-

################################################################################
## Form generated from reading UI file 'ui_main.ui'
##
## Created by: Qt User Interface Compiler version 6.4.2
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
    QSizePolicy, QStatusBar, QTabWidget, QTextBrowser,
    QVBoxLayout, QWidget)

class Ui_MainWindow(object):
    def setupUi(self, MainWindow):
        if not MainWindow.objectName():
            MainWindow.setObjectName(u"MainWindow")
        MainWindow.resize(785, 792)
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
        self.horizontalLayout_9 = QHBoxLayout(self.centralwidget)
        self.horizontalLayout_9.setObjectName(u"horizontalLayout_9")
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
        self.labelModuleUrl.setAlignment(Qt.AlignRight|Qt.AlignTrailing|Qt.AlignVCenter)
        self.labelModuleUrl.setOpenExternalLinks(True)

        self.verticalLayout_4.addWidget(self.labelModuleUrl)


        self.verticalLayout_3.addLayout(self.verticalLayout_4)


        self.verticalLayout.addWidget(self.groupBoxChipView)

        self.labelModulePic = QLabel(self.tabFlash)
        self.labelModulePic.setObjectName(u"labelModulePic")
        sizePolicy = QSizePolicy(QSizePolicy.Preferred, QSizePolicy.Preferred)
        sizePolicy.setHorizontalStretch(0)
        sizePolicy.setVerticalStretch(0)
        sizePolicy.setHeightForWidth(self.labelModulePic.sizePolicy().hasHeightForWidth())
        self.labelModulePic.setSizePolicy(sizePolicy)
        self.labelModulePic.setMaximumSize(QSize(200, 200))
        self.labelModulePic.setCursor(QCursor(Qt.ForbiddenCursor))
        self.labelModulePic.setFrameShape(QFrame.StyledPanel)
        self.labelModulePic.setTextInteractionFlags(Qt.LinksAccessibleByMouse)

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

        self.formLayout_2.setWidget(0, QFormLayout.LabelRole, self.pushButtonCom)

        self.comboBoxCom = QComboBox(self.groupBoxRx)
        self.comboBoxCom.setObjectName(u"comboBoxCom")

        self.formLayout_2.setWidget(0, QFormLayout.FieldRole, self.comboBoxCom)

        self.labelBaudRate = QLabel(self.groupBoxRx)
        self.labelBaudRate.setObjectName(u"labelBaudRate")

        self.formLayout_2.setWidget(1, QFormLayout.LabelRole, self.labelBaudRate)

        self.comboBoxSBaud = QComboBox(self.groupBoxRx)
        self.comboBoxSBaud.setObjectName(u"comboBoxSBaud")
        self.comboBoxSBaud.setEditable(True)

        self.formLayout_2.setWidget(1, QFormLayout.FieldRole, self.comboBoxSBaud)

        self.labelDataBits = QLabel(self.groupBoxRx)
        self.labelDataBits.setObjectName(u"labelDataBits")

        self.formLayout_2.setWidget(2, QFormLayout.LabelRole, self.labelDataBits)

        self.comboBoxDataBits = QComboBox(self.groupBoxRx)
        self.comboBoxDataBits.setObjectName(u"comboBoxDataBits")

        self.formLayout_2.setWidget(2, QFormLayout.FieldRole, self.comboBoxDataBits)

        self.labelParity = QLabel(self.groupBoxRx)
        self.labelParity.setObjectName(u"labelParity")

        self.formLayout_2.setWidget(3, QFormLayout.LabelRole, self.labelParity)

        self.comboBoxParity = QComboBox(self.groupBoxRx)
        self.comboBoxParity.setObjectName(u"comboBoxParity")

        self.formLayout_2.setWidget(3, QFormLayout.FieldRole, self.comboBoxParity)

        self.labelStopBits = QLabel(self.groupBoxRx)
        self.labelStopBits.setObjectName(u"labelStopBits")

        self.formLayout_2.setWidget(4, QFormLayout.LabelRole, self.labelStopBits)

        self.comboBoxStopBits = QComboBox(self.groupBoxRx)
        self.comboBoxStopBits.setObjectName(u"comboBoxStopBits")

        self.formLayout_2.setWidget(4, QFormLayout.FieldRole, self.comboBoxStopBits)


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


        self.horizontalLayout_10.addLayout(self.horizontalLayout_3)


        self.verticalLayout_9.addLayout(self.horizontalLayout_10)

        self.plainTextEditTx = QPlainTextEdit(self.groupBoxTx)
        self.plainTextEditTx.setObjectName(u"plainTextEditTx")
        sizePolicy1 = QSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        sizePolicy1.setHorizontalStretch(0)
        sizePolicy1.setVerticalStretch(0)
        sizePolicy1.setHeightForWidth(self.plainTextEditTx.sizePolicy().hasHeightForWidth())
        self.plainTextEditTx.setSizePolicy(sizePolicy1)
        self.plainTextEditTx.setMaximumSize(QSize(16777215, 100))

        self.verticalLayout_9.addWidget(self.plainTextEditTx)


        self.verticalLayout_2.addWidget(self.groupBoxTx)

        self.tabWidget.addTab(self.tabSerial, "")

        self.horizontalLayout_9.addWidget(self.tabWidget)

        MainWindow.setCentralWidget(self.centralwidget)
        self.menubar = QMenuBar(MainWindow)
        self.menubar.setObjectName(u"menubar")
        self.menubar.setGeometry(QRect(0, 0, 785, 23))
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
        self.pushButtonCom.setText(QCoreApplication.translate("MainWindow", u"COM", None))
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
        self.tabWidget.setTabText(self.tabWidget.indexOf(self.tabSerial), QCoreApplication.translate("MainWindow", u"Serial", None))
        self.menuMain.setTitle(QCoreApplication.translate("MainWindow", u"Main", None))
        self.menuDebug.setTitle(QCoreApplication.translate("MainWindow", u"Debug", None))
    # retranslateUi

