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


class TasmotaLight(Light):
    """Light with the Tasmota firmware as light service.

    Extend the :class:`Light` service to use a tasmota light which can be
    controlled via a web API.
    """

    def __init__(self, location: str, uid: str, room: str,
                 ip: str, dimmable: bool = False) -> None:
        super(TasmotaLight, self).__init__(location, uid, room)
        self.ip_addr = ip
        self.has_dimlevel = dimmable

    def status_setter(self, status: dict) -> None:
        super(TasmotaLight, self).status_setter(status)
        base_cmd = f'http://{self.ip_addr}/cm?cmnd='
        power_value = 'On' if status['state'] else 'Off'
        requests.put(f'{base_cmd}Power%20{power_value}')
        if self.has_dimlevel:
            requests.put(f'{base_cmd}Dimmer%20{self.dimlevel}')
