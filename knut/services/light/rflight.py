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

import rpi_rf

from .light import Light


class RFLight(Light):
    """A RF 433 mHz light service.

    Provide a light service for lights plugged into a RF 433 mHz controllable
    power socket. This class uses the `rpi-rf
    <https://github.com/milaq/rpi-rf>`_ module to control a RF 433 mHz
    transmitter.
    """

    def __init__(self, location: str, uid: str, room: str, gpio: int,
                 code_on: int, code_off: int) -> None:
        """The transmitter at the defined *gpio* is used to send either a
        *code_on* or *code_off* command to switch the power socket.
        """
        super(RFLight, self).__init__(location, uid, room)
        self._gpio = gpio
        self._code_on = code_on
        self._code_off = code_off

    def status_setter(self, status: dict):
        """Send a code via the transmitter depending on the *status*.

        Extend the base class method to send a code depending on the ``'state'``
        key of the status dict.
        """
        logging.debug('Enable TX for device \'%s\'...' % self.uid)
        device = rpi_rf.RFDevice(self._gpio)
        enabled = device.enable_tx()

        if not enabled:
            logging.error('Failed to enable TX for \'%s\''
                          % self.uid)

        if status['state']:
            device.tx_code(self._code_on)
        elif not status['state']:
            device.tx_code(self._code_off)

        device.cleanup()
        del(device)

        super(RFLight, self).status_setter(status)
