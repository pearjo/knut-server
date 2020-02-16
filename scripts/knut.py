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
from knutapis import Light
from knutapis import Temperature
from knutserver import KnutTcpSocket
import argparse
import logging
import sys
import time
import yaml

# global constants
LOGLEVELS = ['DEBUG', 'INFO', 'WARNING', 'ERROR', 'CRITICAL']
config_file = '/etc/knut/knutconfig.yaml'


def load_config(config_file):
    with open(config_file, 'r') as f:
        return yaml.load(f, Loader=yaml.SafeLoader)


def load_service_backend(config, service):
    objects = list()

    for service_id in config[service].keys():
        module = __import__(config[service][service_id]['module'],
                            fromlist=[config[service][service_id]['object']])
        service_object = getattr(module, config[service][service_id]['object'])

        logging.info(('Adding service backend \'%s\' with service id \'%s\' to'
                      + ' service %s.') % (
                          config[service][service_id]['object'],
                          service_id,
                          hex(config[service][service_id]['serviceid'])
                      ))
        location = config[service][service_id]['location']

        try:
            args = config[service][service_id]['args']
        except KeyError:
            args = None

        try:
            kwargs = config[service][service_id]['kwargs']
        except KeyError:
            kwargs = None

        if args and not kwargs:
            objects.append(service_object(location, service_id, *args))
        elif kwargs and not args:
            objects.append(service_object(location, service_id, **kwargs))
        elif args and kwargs:
            objects.append(service_object(location, service_id, *args, **kwargs))
        else:
            objects.append(service_object(location, service_id))

    return objects


def main():
    print(
        'knut.py  Copyright (C) 2020  Joe Pearson\n'
        'This program comes with ABSOLUTELY NO WARRANTY; for details read LICENSE.\n'
        'This is free software, and you are welcome to redistribute it\n'
        'under certain conditions; read LICENSE for details.'
    )

    parser = argparse.ArgumentParser(description='TCP server example.')
    parser.add_argument('--log', dest='logLevel', choices=LOGLEVELS,
                        help='Set the logging level')

    args = parser.parse_args()

    if args.logLevel:
        logging.basicConfig(level=getattr(logging, args.logLevel))

    # load config
    config = load_config(config_file)

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
