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
from knut.apis import *
from knut.core import KnutConfig
from knut.server import KnutTCPServer
import argparse
import coloredlogs
import logging
import sys
import threading

# global constants
LOGLEVELS = ['DEBUG', 'INFO', 'WARNING', 'ERROR', 'CRITICAL']


def main():
    print(
        'knutserver.py  Copyright (C) 2020  Joe Pearson\n'
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

    args = parser.parse_args()

    # setup logging
    logging.basicConfig(level=getattr(logging, args.logLevel))
    logger = logging.getLogger(__name__)
    coloredlogs.install(level=args.logLevel, logger=logger)

    # load config
    config = KnutConfig(args.configFile).config

    # initialize the Knut server
    server = config['server']
    # server = KnutTCPServer()

    # load task module
    task = config['task']
    server.add_api(task)
    task.load_tasks()

    # load temperature module
    try:
        temp = Temperature()
        server.add_api(temp)

        # iterate over all sections, where each section name is a backend ID
        for backend in config['temperature']:
            temp.add_backend(backend)
    except KeyError:
        pass

    # load light module
    try:
        light = Light()
        server.add_api(light)

        for backend in config['lights']:
            light.add_backend(backend)
    except KeyError:
        pass

    # load local module
    if config['local']:
        local = Local()
        server.add_api(local)
        local.set_local(config['local'])

    try:
        with server:
            # logging.debug('Start server on: {}'.format(server.server_address))
            server_thread = threading.Thread(target=server.serve_forever)
            server_thread.daemon = True
            server_thread.start()
            server_thread.join()
    except KeyboardInterrupt:
        logging.debug('Shutting down server...')
        server.shutdown()
        logging.debug('Cheerio!')
        sys.exit(0)


if __name__ == '__main__':
    main()
