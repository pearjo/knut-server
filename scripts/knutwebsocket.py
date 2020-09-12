#!/usr/bin/env python3
"""
Copyright (C) 2020  Joe Pearson

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.

"""
from knut.core import KnutConfig
from knut.server import KnutWebSocket
import argparse
import asyncio
import coloredlogs
import knut.apis
import logging
import sys
import threading

# global constants
LOGLEVELS = ['DEBUG', 'INFO', 'WARNING', 'ERROR', 'CRITICAL']


def main():
    print(
        'knutwebsocket.py  Copyright (C) 2020  Joe Pearson\n'
        'This program comes with ABSOLUTELY NO WARRANTY; for details read LICENSE.\n'
        'This is free software, and you are welcome to redistribute it\n'
        'under certain conditions; read LICENSE for details.\n'
    )

    parser = argparse.ArgumentParser(
        description='Runs the Knut server and all configured services.'
    )
    parser.add_argument('--log', dest='logLevel', choices=LOGLEVELS,
                        default=LOGLEVELS[2],
                        help='Set the logging level')
    parser.add_argument('--conf', dest='configFile',
                        default='/etc/knut/knutserver.yml',
                        help='Set the knut server configuration file')
    parser.add_argument('--address', dest='address',
                        default='localhost',
                        help='Binds the sever to the address')
    parser.add_argument('--port', dest='port',
                        default=8765,
                        help='The port to access the server')

    args = parser.parse_args()

    fmt = '%(levelname)s %(module)s.%(funcName)s: %(message)s'

    # setup logging
    logging.basicConfig(level=getattr(logging, args.logLevel))
    logger = logging.getLogger(__name__)
    coloredlogs.install(level=args.logLevel, logger=logger, fmt=fmt)

    # load config
    config = KnutConfig(args.configFile).config

    socket = KnutWebSocket(args.address, args.port)

    # load task module
    task = config['task']
    task.load_tasks()
    socket.add_api(task)

    # load temperature module
    try:
        temp = knut.apis.Temperature()
        for backend in config['temperature']:
            temp.add_backend(backend)

        socket.add_api(temp)
    except KeyError:
        pass

    # load light module
    try:
        light = knut.apis.Light()
        for backend in config['lights']:
            light.add_backend(backend)

        socket.add_api(light)
    except KeyError:
        pass

    # load local module
    if config['local']:
        local = knut.apis.Local()
        local.set_local(config['local'])
        socket.add_api(local)

    try:
        socket.knut_serve_forever()
    except KeyboardInterrupt:
        logging.debug('Shutting down server...')
        logging.debug('Cheerio!')
        sys.exit(0)


if __name__ == '__main__':
    main()
