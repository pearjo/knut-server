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
from knut.apis import Local
from knut.apis import Task
from knut.apis import Temperature
from knut.server import KnutTCPServer
import argparse
import coloredlogs
import logging
import sys
import threading
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
    config = load_config(args.configFile)

    # initialize the Knut server
    server = KnutTCPServer((config['socket']['ip'], config['socket']['port']))

    try:
        if 'task' in config.keys():
            # load task module
            task = Task()
            server.add_api(task)

            if 'dir' in config['task'].keys():
                # load tasks from file
                task.task_dir = config['task']['dir']
                task.load_tasks()

        if 'temperature' in config.keys():
            # load temperature module
            temp = Temperature()
            server.add_api(temp)

            # iterate over all sections, where each section name is a backend ID
            temp_service_backends = load_service_backend(config, 'temperature')
            for temp_service_backend in temp_service_backends:
                temp.add_backend(temp_service_backend)

        if 'light' in config.keys():
            # load light module
            light = Light()
            server.add_api(light)

            light_service_backends = load_service_backend(config, 'light')
            for light_service_backend in light_service_backends:
                light.add_backend(light_service_backend)

        if 'local' in config.keys():
            # load local module
            local = Local()
            server.add_api(local)

            local_service_locations = load_service_backend(config, 'local')
            for local_service_location in local_service_locations:
                local.set_local(local_service_location)

        with server:
            logging.debug('Start server on: {}'.format(server.server_address))
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
