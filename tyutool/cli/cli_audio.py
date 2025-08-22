#!/usr/bin/env python3
# coding=utf-8

import click

from tyutool.audio import pcm2wav
from tyutool.audio import play_sound
from tyutool.util.util import set_clis, get_logger


@click.command()
@click.option('-i', '--input',
              type=str, required=True,
              help="Input pcm file")
@click.option('-o', '--output',
              type=str, required=False,
              help="Output wav file")
@click.option('-s', '--sample',
              type=int, default=16000,
              help="Sample rate")
@click.option('-b', '--bits',
              type=int, default=16,
              help="Bits Per Sample")
@click.option('-c', '--channels',
              type=int, default=1,
              help="Channels")
def pcm2wav_cli(input, output,
                sample, bits, channels):
    logger = get_logger()
    logger.debug(f"input: {input}")
    logger.debug(f"output: {output}")
    logger.debug(f"sample: {sample}")
    logger.debug(f"bits: {bits}")
    logger.debug(f"channels: {channels}")

    output_default = f"{input}.wav"
    if input.endswith(".pcm"):
        output_default = f"{input[:-4]}.wav"
    output_file = output or output_default

    pcm2wav(input, output_file, sample, bits, channels)
    pass


@click.command()
@click.option('-f', '--file',
              type=str, required=True,
              help="Input pcm file")
def play_cli(file):
    logger = get_logger()
    logger.debug(f"file: {file}")
    play_sound(file)
    pass


CLIS = {
    "play": play_cli,
    "pcm2wav": pcm2wav_cli,
}


@click.command(cls=set_clis(CLIS),
               help="Audio Conversion Tools.",
               context_settings=dict(help_option_names=["-h", "--help"]))
def cli():
    pass
