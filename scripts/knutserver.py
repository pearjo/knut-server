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
from knut.apis import Light
from knut.apis import Temperature
from knut.server import KnutTcpSocket
import argparse
import coloredlogs
import logging
import sys
import time
import yaml

# global constants
LOGLEVELS = ['DEBUG', 'INFO', 'WARNING', 'ERROR', 'CRITICAL']


def load_config(config_file):
    with open(config_file, 'r') as f:
        return yaml.load(f, Loader=yaml.SafeLoader)


def load_service_backend(config, service):
    objects = list()

    for unique_name in config[service].keys():
        module = __import__(config[service][unique_name]['module'],
                            fromlist=[config[service][unique_name]['object']])
        service_object = getattr(module,
                                 config[service][unique_name]['object'])

        logging.info(('Adding service backend \'%s\' with unique name \'%s\' to'
                      + ' service %s.') % (
                          config[service][unique_name]['object'],
                          unique_name,
                          hex(config[service][unique_name]['serviceid'])
        ))
        location = config[service][unique_name]['location']

        try:
            args = config[service][unique_name]['args']
        except KeyError:
            args = None

        try:
            kwargs = config[service][unique_name]['kwargs']
        except KeyError:
            kwargs = None

        if args and not kwargs:
            objects.append(service_object(location, unique_name, *args))
        elif kwargs and not args:
            objects.append(service_object(location, unique_name, **kwargs))
        elif args and kwargs:
            objects.append(service_object(
                location, unique_name, *args, **kwargs))
        else:
            objects.append(service_object(location, unique_name))

    return objects


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
                        default=LOGLEVELS[0],
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
    config = load_config(args.configFile)

    # initialize the Knut TCP server socket
    socket = KnutTcpSocket(config['socket']['ip'], config['socket']['port'])

    try:
        # load all service modules
        # load temperature module
        temp = Temperature(socket)
        socket.add_service(temp)

        # load light module
        light = Light(socket)
        socket.add_service(light)

        # iterate over all sections, where each section name is a backend ID
        temp_service_backends = load_service_backend(config, 'temperature')
        for temp_service_backend in temp_service_backends:
            temp.add_backend(temp_service_backend)

        light_service_backends = load_service_backend(config, 'light')
        for light_service_backend in light_service_backends:
            light.add_backend(light_service_backend)

        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        logging.debug('Close open server sockets.')
        socket.exit()
        sys.exit(0)


if __name__ == '__main__':
    main()
