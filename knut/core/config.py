# -*- coding: utf-8 -*-

# Copyright (C) 2020  Joe Pearson
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.

import logging

import yaml

from .base import KnutObject
import knut.server.tcpserver
import knut.services.local


class KnutConfig(KnutObject):
    """Knut configuration."""

    config = {
        'lights': [],
        'local': None,
        'server': None,
        'task': None,
        'temperature': []
    }
    """The configuration dictionary."""

    def __init__(self, file: str='/etc/knutserver.yml') -> None:
        """Load the configuration from a *file*.

        The :attr:`config` dictionary is filled from the configuration file. The
        Knut objects in the YAML file with the tag ``!knutobject`` are
        initialize, too. If no file is found, a :meth:`failsafe()` configuration
        is loaded.

        To load a Knut object from the configuration, it must be configured as
        following:

        .. code-block:: yaml

           !knutobject
             module: module
             class: Class
             attribute: value
             ...

        The keys ``module`` and ``class`` are mandatory and specify the Class
        and the module containing it to load. The following keys are the
        arguments of the classes ``__init__()`` method. For example, the
        :class:`knut.server.tcpserver.KnutTCPServer` would be configured as
        following:

        .. code-block:: yaml

           !knutobject
             module: knut.server.tcpserver
             class: KnutTCPServer
             address: 127.0.0.1
             port: 8080

        For details about the content of the configuration file, see
        :ref:`config`.

        """
        self.file = file
        self.__load_config_file()

    def __load_config_file(self) -> None:
        """Loads all configurations from a file."""
        try:
            with open(self.file, 'r') as f:
                config = yaml.load(f, Loader=yaml.SafeLoader)
                for key, item in config.items():
                    self.config[key] = item
        except (FileNotFoundError, TypeError) as e:
            logging.error('Failed to load configuration: {}: {}'
                          .format(self.file, e))
            logging.warning('Using fail-safe configuration.')
            self.config = self.failsafe()

    def failsafe(self) -> dict:
        """Returns a fail-safe configuration."""
        return {
            'server': knut.server.tcpserver.KnutTCPServer(),
            'task': knut.apis.Task(),
            'temperature': list(),
            'lights': list(),
            'local': knut.services.local.Local()
        }
