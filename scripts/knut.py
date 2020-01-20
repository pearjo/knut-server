#!/usr/bin/env python3
import logging
import sys
import argparse
import time
import pathlib
import configparser
import yaml
import knutcore.utility
from knutserver import KnutTcpSocket
from knutapis import Temperature
from knutapis import Light

# global variables
interrupt = False  # interrupt and exit gracefully if true

# global constants
LOGLEVELS = ['DEBUG', 'INFO', 'WARNING', 'ERROR', 'CRITICAL']
config_file = '/etc/knut/knutconfig.yaml'


def load_config(config_file: str) -> dict:
    with open(config_file, 'r') as f:
        return yaml.load(f, Loader=yaml.FullLoader)


def load_service_backend(config: dict, service: str) -> list():
    objects = list()

    for id in config[service].keys():
        module = __import__(config[service][id]['module'],
                            fromlist=[config[service][id]['object']])
        service_object = getattr(module, config[service][id]['object'])

        logging.info(('Adding service backend \'%s\' with ID \'%s\' to'
                      + ' service %s.') % (config[service][id]['object'],
                                           id,
                                           hex(config[service][id]['serviceid'])))
        location = config[service][id]['location']

        try:
            args = config[service][id]['args']
        except KeyError:
            args = None

        try:
            kwargs = config[service][id]['kwargs']
        except KeyError:
            kwargs = None

        if args and not kwargs:
            objects.append(service_object(location, id, *args))
        elif kwargs and not args:
            objects.append(service_object(location, id, **kwargs))
        elif args and kwargs:
            objects.append(service_object(location, id, *args, **kwargs))
        else:
            objects.append(service_object(location, id))

    return objects


def main() -> None:
    parser = argparse.ArgumentParser(description='TCP server example.')
    parser.add_argument('--log', dest='logLevel', choices=LOGLEVELS,
                        help='Set the logging level')

    args = parser.parse_args()

    if args.logLevel:
        logging.basicConfig(level=getattr(logging, args.logLevel))

    # handle interrupt signal
    utility = knutcore.utility.KnutUtility()

    # load config
    config = load_config(config_file)

    # initialize the Knut TCP server socket
    socket = KnutTcpSocket(config['socket']['ip'], config['socket']['port'])

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

    # start main loop
    while True:
        if utility.interrupt:
            # close open sockets
            logging.debug('Close open server sockets.')
            socket.exit()

            logging.info('Goodbye Knut, see you soon!')
            sys.exit(0)

        time.sleep(0.2)


if __name__ == '__main__':
    main()
