#!/usr/bin/env python
# -*- coding: utf-8 -*-

import os
import json
import logging

from PySide6 import QtWidgets
from PySide6.QtCore import (
    Qt, QRegularExpression, QObject, Signal, QTimer, QUrl
)
from PySide6.QtGui import QRegularExpressionValidator
from PySide6.QtWidgets import (
    QTextEdit, QWidget, QHBoxLayout, QVBoxLayout, QPushButton, QLabel, QFrame
)

from tyutool.ai_debug import WebAIDebugMonitor

# 屏蔽import QtMultimedia 时的警告
os.environ["QT_LOGGING_RULES"] = "qt.multimedia.symbolsresolver.warning=false;\
qt.multimedia.ffmpeg*=false"
from PySide6 import QtMultimedia


class WebDisplayHookSignal(QObject):
    hook_signal = Signal(object, str)
    pass


class WebGuiDataDisplayHook:
    def __init__(self, ui, logger):
        self.ui = ui
        self.logger = logger
        self.hook_signal = WebDisplayHookSignal()
        self.hook_signal.hook_signal.connect(self.show_packet)
        self.pack_count = {
            "text": 0,
            "audio": 0,
            "picture": 0,
            "video": 0,
        }
        self._update_count()
        pass

    def _update_count(self):
        self.ui.labelWDTextCount.setText(f"{self.pack_count['text']}")
        self.ui.labelWDAudioCount.setText(f"{self.pack_count['audio']}")
        self.ui.labelWDPictureCount.setText(f"{self.pack_count['picture']}")
        self.ui.labelWDVideoCount.setText(f"{self.pack_count['video']}")
        pass

    def clear_count(self):
        self.pack_count["text"] = 0
        self.pack_count["audio"] = 0
        self.pack_count["picture"] = 0
        self.pack_count["video"] = 0
        self._update_count()
        pass

    def hook(self, packet, msg):
        self.hook_signal.hook_signal.emit(packet, msg)
        pass

    def _scroll_to_bottom(self, scroll_area):
        scroll_bar = scroll_area.verticalScrollBar()
        scroll_bar.setValue(scroll_bar.maximum())
        QTimer.singleShot(0, lambda: scroll_bar.setValue(scroll_bar.maximum()))
        pass

    def _add_msg_to_scroll(self,
                           scroll_area, scroll_content, scroll_layout,
                           msg):
        scroll_bar = scroll_area.verticalScrollBar()
        scroll_at_bottom = scroll_bar.value() >= (scroll_bar.maximum() - 1)

        text_edit = QTextEdit()
        text_edit.setText(msg)
        text_edit.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        text_edit.setReadOnly(True)
        text_edit.setLineWrapMode(QTextEdit.WidgetWidth)

        scroll_layout.addWidget(text_edit)
        text_edit.document().setTextWidth(text_edit.width())
        document_height = text_edit.document().size().height()
        text_edit.setMinimumHeight(int(document_height) + 2)
        text_edit.setMaximumHeight(int(document_height) + 2)
        scroll_content.updateGeometry()
        scroll_area.updateGeometry()

        if scroll_at_bottom:
            QTimer.singleShot(10, lambda: self._scroll_to_bottom(scroll_area))
        pass

    def _add_audio_to_scroll(self,
                             scroll_area, scroll_content, scroll_layout,
                             packet):
        stream_flag = packet.get('stream_flag', 0)
        stream_save = packet.get('stream_save', "")
        if (3 != stream_flag) or (not os.path.exists(stream_save)):
            return

        # Create audio player widget
        audio_widget = self._create_audio_player_widget(packet)

        # Add to scroll layout
        scroll_bar = scroll_area.verticalScrollBar()
        scroll_at_bottom = scroll_bar.value() >= (scroll_bar.maximum() - 1)

        # Remove existing stretch if any
        for i in reversed(range(scroll_layout.count())):
            item = scroll_layout.itemAt(i)
            if item and item.spacerItem():
                scroll_layout.removeItem(item)
                break

        # Add audio widget
        scroll_layout.addWidget(audio_widget)

        # Add stretch to push all widgets to top
        scroll_layout.addStretch()

        scroll_content.updateGeometry()
        scroll_area.updateGeometry()

        if scroll_at_bottom:
            QTimer.singleShot(10, lambda: self._scroll_to_bottom(scroll_area))
        pass

    def _create_audio_player_widget(self, packet):
        """Create audio player widget with controls and info"""
        main_widget = QWidget()
        main_widget.setFixedHeight(60)  # Fixed height for long bar shape
        main_layout = QVBoxLayout(main_widget)
        main_layout.setContentsMargins(2, 2, 2, 2)
        main_layout.setSpacing(0)

        # Create frame for styling
        frame = QFrame()
        frame.setFrameStyle(QFrame.Box)
        frame.setStyleSheet("QFrame { border: 1px solid #555555; \
background-color: #2a2a2a; border-radius: 5px; }")
        frame.setFixedHeight(56)

        # Main horizontal layout inside frame
        frame_layout = QHBoxLayout(frame)
        frame_layout.setContentsMargins(15, 8, 15, 8)
        # Increased spacing for less compact layout
        frame_layout.setSpacing(25)

        # Play/Stop button - leftmost
        play_button = QPushButton("▶ Play")
        play_button.setFixedSize(75, 35)
        play_button.setStyleSheet("""
            QPushButton {
                background-color: #4CAF50;
                color: white;
                border: none;
                border-radius: 4px;
                font-weight: bold;
                font-size: 11px;
            }
            QPushButton:hover {
                background-color: #45a049;
            }
            QPushButton:pressed {
                background-color: #3e8e41;
            }
        """)
        frame_layout.addWidget(play_button)

        # File path label (highlighted)
        file_path = packet.get('stream_save', '')
        file_name = os.path.basename(file_path) if file_path else 'unknown'
        path_label = QLabel(f"File: {file_name}")
        path_label.setStyleSheet("QLabel { color: #FFD700; \
font-weight: bold; font-size: 13px; \
background-color: #404040; padding: 4px 8px; \
border-radius: 3px; }")
        path_label.setToolTip(file_path)  # Full path in tooltip
        path_label.setMinimumWidth(150)
        path_label.setTextInteractionFlags(Qt.TextSelectableByMouse)
        frame_layout.addWidget(path_label)

        # Direction
        direction = packet.get('direction', 'unknown')
        dir_label = QLabel(f"Dir: {direction}")
        dir_label.setStyleSheet("QLabel { color: #2196F3; font-size: 14px; }")
        dir_label.setFixedWidth(60)
        dir_label.setTextInteractionFlags(Qt.TextSelectableByMouse)
        frame_layout.addWidget(dir_label)

        # Size
        size = packet.get('size_sum', 0)
        size_str = f"{size} bytes" if size else 'N/A'
        size_label = QLabel(f"Size: {size_str}")
        size_label.setStyleSheet("QLabel { color: #4CAF50; \
font-weight: bold; font-size: 14px; }")
        size_label.setFixedWidth(150)
        size_label.setTextInteractionFlags(Qt.TextSelectableByMouse)
        frame_layout.addWidget(size_label)

        # Timestamp
        timestamp = packet.get('timestamp', 0)
        time_label = QLabel(f"Time: {timestamp}")
        time_label.setStyleSheet("QLabel { color: #FF9800; font-size: 14px; }")
        time_label.setFixedWidth(170)
        time_label.setTextInteractionFlags(Qt.TextSelectableByMouse)
        frame_layout.addWidget(time_label)

        # Add stretch to push remaining elements to the right
        frame_layout.addStretch()

        main_layout.addWidget(frame)

        # Setup audio player
        audio_player = QtMultimedia.QMediaPlayer()
        audio_output = QtMultimedia.QAudioOutput()
        audio_player.setAudioOutput(audio_output)

        # Store references in widget
        main_widget.audio_player = audio_player
        main_widget.audio_output = audio_output
        main_widget.play_button = play_button
        main_widget.is_playing = False

        # Set audio source
        if file_path and os.path.exists(file_path):
            audio_player.setSource(QUrl.fromLocalFile(file_path))

        # Connect play button
        def toggle_playback():
            if main_widget.is_playing:
                audio_player.stop()
                play_button.setText("▶ Play")
                play_button.setStyleSheet("""
                    QPushButton {
                        background-color: #4CAF50;
                        color: white;
                        border: none;
                        border-radius: 4px;
                        font-weight: bold;
                        font-size: 11px;
                    }
                    QPushButton:hover {
                        background-color: #45a049;
                    }
                    QPushButton:pressed {
                        background-color: #3e8e41;
                    }
                """)
                main_widget.is_playing = False
            else:
                audio_player.play()
                play_button.setText("⏹ Stop")
                play_button.setStyleSheet("""
                    QPushButton {
                        background-color: #f44336;
                        color: white;
                        border: none;
                        border-radius: 4px;
                        font-weight: bold;
                        font-size: 11px;
                    }
                    QPushButton:hover {
                        background-color: #da190b;
                    }
                    QPushButton:pressed {
                        background-color: #c62828;
                    }
                """)
                main_widget.is_playing = True
            pass

        play_button.clicked.connect(toggle_playback)

        # Auto-stop when finished
        def on_playback_finished():
            play_button.setText("▶ Play")
            play_button.setStyleSheet("""
                QPushButton {
                    background-color: #4CAF50;
                    color: white;
                    border: none;
                    border-radius: 4px;
                    font-weight: bold;
                    font-size: 11px;
                }
                QPushButton:hover {
                    background-color: #45a049;
                }
                QPushButton:pressed {
                    background-color: #3e8e41;
                }
            """)
            main_widget.is_playing = False
            pass

        def _media_status_changed(status):
            if status == QtMultimedia.QMediaPlayer.EndOfMedia:
                on_playback_finished()
            pass

        audio_player.mediaStatusChanged.connect(
            _media_status_changed
        )

        return main_widget

    def show_packet(self, packet, msg):
        packet_type = packet['type']
        if packet_type == 31:  # Audio
            scroll_area = self.ui.scrollAreaWDAudio
            scroll_content = self.ui.scrollAreaWidgetContentsWDAudio
            scroll_layout = self.ui.verticalLayout_23
            self.pack_count["audio"] += 1
            self._add_audio_to_scroll(
                scroll_area, scroll_content, scroll_layout, packet
            )
        elif packet_type == 34:  # Text
            self.pack_count["text"] += 1
            scroll_area = self.ui.scrollAreaWDText
            scroll_content = self.ui.scrollAreaWidgetContentsWDText
            scroll_layout = self.ui.verticalLayout_22
            text_msg = packet["text_content"]
            self._add_msg_to_scroll(
                scroll_area, scroll_content, scroll_layout, text_msg
            )
        elif packet_type == 30:  # Video
            # scroll_area = self.ui.scrollAreaWDVideo
            # scroll_content = self.ui.scrollAreaWidgetContentsWDVideo
            # scroll_layout = self.ui.verticalLayout_25
            self.pack_count["video"] += 1
        elif packet_type == 32:  # Image
            # scroll_area = self.ui.scrollAreaWDPicture
            # scroll_content = self.ui.scrollAreaWidgetContentsWDPicture
            # scroll_layout = self.ui.verticalLayout_24
            self.pack_count["picture"] += 1

        self._update_count()

        scroll_area = self.ui.scrollAreaWDMsg
        scroll_content = self.ui.scrollAreaWidgetContentsWDMsg
        scroll_layout = self.ui.verticalLayout_27
        self._add_msg_to_scroll(
            scroll_area, scroll_content, scroll_layout, msg
        )
        pass


class WebLogSignal(QObject):
    log_message = Signal(str)
    pass


class WebQTextEditHandler(logging.Handler):
    def __init__(self, text_edit):
        super().__init__()
        self.text_edit = text_edit
        self.log_signal = WebLogSignal()
        self.log_signal.log_message.connect(self._append_log)
        pass

    def emit(self, record):
        msg = self.format(record)
        self.log_signal.log_message.emit(msg)
        pass

    def _append_log(self, msg):
        self.text_edit.append(msg)
        pass


class WebDebugGUI(QtWidgets.QMainWindow):
    def __init__(self):
        super().__init__()
        self.wd_monitor = None
        self.wd_logger = None
        self.wd_display_hook = None
        pass

    def _setupWebLogger(self):
        logger = logging.getLogger("tyut_web_debug")
        logger.setLevel(logging.INFO)
        log_format = "[%(levelname)s]: %(message)s"
        lf = logging.Formatter(fmt=log_format)
        handler = WebQTextEditHandler(self.ui.textBrowserWD)
        handler.setFormatter(lf)
        logger.addHandler(handler)
        logger.info("tyut_web_debug init done.")
        self.wd_logger = logger
        pass

    def wdCacheSave(self):
        cache_data = {
            "ip": self.ui.lineEditWDIP.text(),
            "port": self.ui.lineEditWDPort.text(),
            "save": self.ui.lineEditWDSave.text(),
            "enable_text": self.ui.checkBoxWDText.isChecked(),
            "enable_audio": self.ui.checkBoxWDAudio.isChecked(),
            "enable_picture": self.ui.checkBoxWDPicture.isChecked(),
            "enable_video": self.ui.checkBoxWDVideo.isChecked(),
        }
        json_str = json.dumps(cache_data, indent=4, ensure_ascii=False)
        with open(self.web_debug_cache, 'w', encoding='utf-8') as f:
            f.write(json_str)
        pass

    def wdCacheUse(self):
        cache_data = {}
        if os.path.exists(self.web_debug_cache):
            f = open(self.web_debug_cache, 'r', encoding='utf-8')
            cache_data = json.load(f)
            f.close()

        ip = cache_data.get('ip', "localhost")
        port = cache_data.get('port', "5055")
        save = cache_data.get('save', "web_ai_debug")
        self.ui.lineEditWDIP.setText(ip)
        self.ui.lineEditWDPort.setText(port)
        self.ui.lineEditWDSave.setText(save)

        self.ui.checkBoxWDText.setChecked(
            cache_data.get('enable_text', False)
        )
        self.ui.checkBoxWDAudio.setChecked(
            cache_data.get('enable_audio', False)
        )
        self.ui.checkBoxWDPicture.setChecked(
            cache_data.get('enable_picture', False)
        )
        self.ui.checkBoxWDVideo.setChecked(
            cache_data.get('enable_video', False)
        )
        pass

    def webDebugUiSetup(self):
        self.web_debug_cache = os.path.join(
            self.cache_dir, "web_debug.cache"
        )
        self.ui.textBrowserWD.setStyleSheet(
            "color: rgb(37, 214, 78); background-color: rgb(35, 35, 35);"
        )

        reg_port = QRegularExpression('[0-9]+$')
        validator_port = QRegularExpressionValidator(reg_port)
        self.ui.lineEditWDPort.setValidator(validator_port)

        self.ui.pushButtonWDConnect.clicked.connect(
            self.pushButtonWDConnectClicked
        )
        self.ui.pushButtonWDClear.clicked.connect(
            self.pushButtonWDClearClicked
        )
        self.ui.checkBoxWDDebug.stateChanged.connect(
            self.checkBoxWDDebugChecked
        )

        self._setupWebLogger()
        self.wdCacheUse()
        pass

    def pushButtonWDConnectClicked(self):
        event = []
        if self.ui.checkBoxWDText.isChecked():
            event.append('t')
        if self.ui.checkBoxWDAudio.isChecked():
            event.append('a')
        if self.ui.checkBoxWDPicture.isChecked():
            event.append('p')
        if self.ui.checkBoxWDVideo.isChecked():
            event.append('v')

        ip = self.ui.lineEditWDIP.text()
        port = int(self.ui.lineEditWDPort.text())
        save = self.ui.lineEditWDSave.text()
        if self.wd_display_hook:
            display_hook = self.wd_display_hook
        else:
            display_hook = WebGuiDataDisplayHook(self.ui, self.wd_logger)

        monitor = WebAIDebugMonitor(
            ip, port, event, save, self.wd_logger,
            display_hook
        )

        # Connect to server
        if not monitor.connect():
            self.wd_logger.error("Open socket success.")
            return

        self.wd_monitor = monitor
        self.wd_display_hook = display_hook
        self.wdCacheSave()

        self.ui.pushButtonWDConnect.setText("Quit")
        self.ui.pushButtonWDConnect.clicked.disconnect()
        self.ui.pushButtonWDConnect.clicked.connect(
            self.pushButtonWDDisconnectClicked
        )
        self.wd_logger.info("Open socket success.")
        pass

    def pushButtonWDDisconnectClicked(self):
        self.wd_monitor.disconnect()
        self.wd_monitor = None

        self.ui.pushButtonWDConnect.setText("Connect")
        self.ui.pushButtonWDConnect.clicked.disconnect()
        self.ui.pushButtonWDConnect.clicked.connect(
            self.pushButtonWDConnectClicked
        )
        self.wd_logger.info("Quit success.")
        pass

    def _clear_scroll_area(self, scroll_area, scroll_content, scroll_layout):
        while scroll_layout.count():
            item = scroll_layout.takeAt(0)
            widget = item.widget()
            if widget:
                widget.deleteLater()
        scroll_content.updateGeometry()
        scroll_area.updateGeometry()
        pass

    def pushButtonWDClearClicked(self):
        self.ui.textBrowserWD.clear()
        self._clear_scroll_area(
            self.ui.scrollAreaWDText,
            self.ui.scrollAreaWidgetContentsWDText,
            self.ui.verticalLayout_22)
        self._clear_scroll_area(
            self.ui.scrollAreaWDAudio,
            self.ui.scrollAreaWidgetContentsWDAudio,
            self.ui.verticalLayout_23)
        self._clear_scroll_area(
            self.ui.scrollAreaWDPicture,
            self.ui.scrollAreaWidgetContentsWDPicture,
            self.ui.verticalLayout_24)
        self._clear_scroll_area(
            self.ui.scrollAreaWDVideo,
            self.ui.scrollAreaWidgetContentsWDVideo,
            self.ui.verticalLayout_25)

        self.wd_display_hook.clear_count()
        pass

    def checkBoxWDDebugChecked(self, state):
        if self.ui.checkBoxWDDebug.isChecked():
            self.wd_logger.setLevel(logging.DEBUG)
            self.wd_logger.debug("Debug on.")
        else:
            self.wd_logger.setLevel(logging.INFO)
            self.wd_logger.info("Debug off.")
        pass
