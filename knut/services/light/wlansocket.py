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

import requests

from .light import Light


class WlanSocket(Light):
    """WLAN power socket as light service.

    Extend the :class:`Light` service to use a power socket which can be
    controlled via a web API.
    """

    def __init__(self, location: str, uid: str, room: str,
                 cmd_on: str, cmd_off: str) -> None:
        """Switch the socket on or off with the HTTP PUT commands *cmd_on* and
        *cmd_off*.
        """
        self.cmd_off = cmd_off
        """HTTP PUT command to switch the light off."""

        self.cmd_on = cmd_on
        """HTTP PUT command to switch the light on."""

        super(WlanSocket, self).__init__(location, uid, room)

    def status_setter(self, status: dict) -> None:
        """Extend the status setter to send either :attr:`cmd_off` or :attr:`cmd_on`
        depending in the ``'state'`` key of the *status* dict.
        """
        cmd = self.cmd_on if status['state'] else self.cmd_off
        requests.put(cmd)
        super(WlanSocket, self).status_setter(status)
