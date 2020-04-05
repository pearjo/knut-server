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
from knut.services import Light
import logging

try:
    import rpi_rf
except RuntimeError:
    logging.critical('Failed to load module for RFLight.')
    rpi_rf = None


class RFLight(Light):
    """RF 433 mHz light service.

    This light service is used for lights behind a RF 433 mHz socket.  To work,
    this class needs a transmitter installed at a *gpio*.  Also, the decimal
    *code_on* and *code_off* to switch the socket is needed.
    For more details have a look at `rpi-rf <https://github.com/milaq/rpi-rf>`_.
    """
    def __init__(self, location, unique_name, room, gpio, code_on, code_off):
        super(RFLight, self).__init__(location, unique_name, room)
        self._gpio = gpio
        self._code_on = code_on
        self._code_off = code_off

    def status_setter(self, status):
        if rpi_rf:
            logging.debug('Enable TX for device \'%s\'...' % self.unique_name)
            device = rpi_rf.RFDevice(self._gpio)
            enabled = device.enable_tx()

            if not enabled:
                logging.error('Failed to enable TX for \'%s\''
                              % self.unique_name)

            if status['state']:
                device.tx_code(self._code_on)
            elif not status['state']:
                device.tx_code(self._code_off)

            device.cleanup()
            del(device)
        else:
            logging.error('\'%s\' needs to run on a Raspberry Pi.'
                          % self.unique_name)

        super(RFLight, self).status_setter(status)
