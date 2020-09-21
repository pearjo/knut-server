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
from .light import Light


class DummyLight(Light):
    """A dummy light for mock purpose.

    .. warning:: This object is only for mock purpose and has no functionality.
    """

    def __init__(self, location, uid, room, dimlevel=False,
                 color=False, temperature=False, color_cold='#f5faf6',
                 color_warm='#efd275'):
        super(DummyLight, self).__init__(location, uid, room)
        self.has_dimlevel = dimlevel
        self.has_color = color
        self.has_temperature = temperature

        self.temperature = 80
        self.dimlevel = 0
        self.state = False
        self.color = '#ff1122'

        # set color temperature limits if provided
        if self.has_temperature:
            self.color_cold = color_cold
            self.color_warm = color_warm
