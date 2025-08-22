#!/usr/bin/env python3
# coding=utf-8

import os
import sqlite3


class SaveDatabase:
    def __init__(self, save_db, logger):
        os.makedirs(os.path.dirname(save_db), exist_ok=True)
        if os.path.exists(save_db):
            os.remove(save_db)

        self.save_db = save_db
        self.logger = logger
        self.logger.debug("SaveDatabase init.")
        pass

    def _save_text_pack(self, packet):
        '''
        packet = {
            'direction': direction,
            'type': packet_type,
            'type_name': type_name,
            'attributes': attributes,
            'payload': packet_payload,
            'data_id': data_id,
            'stream_flag': stream_flag,
            'stream_status': status,
            'text_content': text_content,
            'size': length,
        }
        '''
        conn = sqlite3.connect(self.save_db)
        cursor = conn.cursor()
        create_table_sql = """
        CREATE TABLE IF NOT EXISTS text_pack (
            id INTEGER PRIMARY KEY,
            direction INTEGER,
            type INTEGER,
            type_name TEXT,
            attributes TEXT,
            data_id INTEGER,
            stream_flag INTEGER,
            stream_status TEXT,
            text_content TEXT,
            size INTEGER
        );
        """
        cursor.execute(create_table_sql)

        sample_data = (
            packet["direction"],           # direction (INTEGER)
            packet["type"],                # type (INTEGER)
            packet["type_name"],           # type_name (TEXT)
            str(packet["attributes"]),     # attributes (TEXT)
            packet["data_id"],             # data_id (INTEGER)
            packet["stream_flag"],         # stream_flag (INTEGER)
            packet["stream_status"],       # stream_status (TEXT)
            packet["text_content"],        # text_content (TEXT)
            packet["size"],                # size (INTEGER)
        )

        insert_sql = """
        INSERT INTO text_pack
        (direction, type, type_name, attributes, data_id,
        stream_flag, stream_status, text_content, size)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?);
        """
        cursor.execute(insert_sql, sample_data)

        conn.commit()
        conn.close()
        pass

    def _save_audio_pack(self, packet):
        '''
        packet = {
            'direction': direction,
            'type': packet_type,
            'type_name': type_name,
            'attributes': attributes,
            'payload': packet_payload,
            'data_id': data_id,
            'stream_flag': stream_flag,
            'stream_status': status,
            'timestamp': timestamp,
            'pts': pts,
            'media_payload': media_payload,
            'size': length,
            'stream_id': MEDIA_STREAM_ID,  # 工具自定义字段
        }
        '''
        conn = sqlite3.connect(self.save_db)
        cursor = conn.cursor()
        create_table_sql = """
        CREATE TABLE IF NOT EXISTS audio_pack (
            id INTEGER PRIMARY KEY,
            direction INTEGER,
            type INTEGER,
            type_name TEXT,
            attributes TEXT,
            data_id INTEGER,
            stream_flag INTEGER,
            stream_status TEXT,
            timestamp TIMESTAMP,
            pts TIMESTAMP,
            size INTEGER,
            stream_id TEXT
        );
        """
        cursor.execute(create_table_sql)

        sample_data = (
            packet["direction"],           # direction (INTEGER)
            packet["type"],                # type (INTEGER)
            packet["type_name"],           # type_name (TEXT)
            str(packet["attributes"]),     # attributes (TEXT)
            packet["data_id"],             # data_id (INTEGER)
            packet["stream_flag"],         # stream_flag (INTEGER)
            packet["stream_status"],       # stream_status (TEXT)
            packet["timestamp"],           # timestamp (TIMESTAMP)
            packet["pts"],                 # pts (TIMESTAMP)
            packet["size"],                # size (INTEGER)
            packet["stream_id"],           # stream_id (TEXT)
        )

        insert_sql = """
        INSERT INTO audio_pack
        (direction, type, type_name, attributes, data_id,
        stream_flag, stream_status, timestamp, pts, size,
        stream_id)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);
        """
        cursor.execute(insert_sql, sample_data)

        conn.commit()
        conn.close()
        pass

    def save(self, packet):
        packet_type = packet['type']
        if packet_type == 31:  # Audio
            self._save_audio_pack(packet)
        elif packet_type == 34:  # Text
            self._save_text_pack(packet)
        pass
