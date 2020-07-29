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
from events import Events
import logging


class Light(Events):
    """Base class for light services."""

    def __init__(self, location, uid, room):
        """The light has an *uid* and *location* within a *room*.

        If the state of the light changes, e.g. when calling
        :meth:`status_setter()`, the method :meth:`on_change()` is called with
        the *uid* as argument. Any listener can register upon this
        event.

        """
        self.has_color = bool()
        self.has_dimlevel = bool()
        self.has_temperature = bool()

        self.location = location
        """The location inside a :attr:`room`."""
        self.uid = uid
        """The unique identifier used by the Knut system."""
        self.room = room
        """The room in which the light is located."""

        self.state = bool()
        """The state of the light as boolean."""
        self.dimlevel = int()
        """The dimlevel as percentage value where at 0 the light is off and 100,
        it's full on."""
        # used to restore dim level after state change
        self.saved_dimlevel = int()
        self.color = str()
        """The light color as hex code e.g. ``#000000``."""
        self.temperature = int()
        """The light temperature as percentage value where 0 is the coldest
        and 100 the warmest color temperature."""
        self.color_cold = str()
        """A color that represents the light when the temperature is 0."""
        self.color_warm = str()
        """A color that represents the light when the temperature is 100."""

        # Call on_change as method with the uid as argument to notify
        # listening methods.
        self.__events__ = ('on_change')

    def status(self):
        return {
            'id': self.uid,
            'location': self.location,
            'room': self.room,
            'state': self.state,
            'hasTemperature': self.has_temperature,
            'hasDimlevel': self.has_dimlevel,
            'hasColor': self.has_color,
            'temperature': int(self.temperature) if self.has_temperature else None,
            'colorCold': self.color_cold if self.has_temperature else None,
            'colorWarm': self.color_warm if self.has_temperature else None,
            'dimlevel': int(self.dimlevel) if self.has_dimlevel else None,
            'color': self.color if self.has_color else None
        }

    def status_setter(self, status):
        """Applies the *status* to the back-end.

        The *status* dictionary must have ``'state'`` as key and can have the
        following keys additional:

        * The ``'dimlevel'`` :attr:`dimlevel`.
        * The lights ``'temperature'`` :attr:`temperature`.
        * The light ``'color'`` :attr:`light`.

        """
        state = status['state']
        if self.has_dimlevel:
            dimlevel = None
            if 'dimlevel' in status.keys():
                dimlevel = status['dimlevel']

            if dimlevel:
                # If the back-end state is false and either the status state is
                # true or the status dim level is > 0, switch back-end state to
                # true and set the last active dim level. Else, if the back-end
                # state is true and either the status dim level is 0 or state is
                # false, switch the back-end state to false and the dim level to
                # 0. If none of the two cases are given, the state and dim level
                # of the back-end are set from the status.
                if 0 <= dimlevel <= 100:
                    if (self.state is False and
                        (state is True or
                         dimlevel > 0)):
                        self.state = True
                        self.dimlevel = (
                            dimlevel if dimlevel > 0 else self.saved_dimlevel
                        )
                    elif (self.state is True and
                          (state is False
                           or dimlevel == 0)):
                        self.state = False
                        self.saved_dimlevel = (
                            self.dimlevel if self.dimlevel > 0 else 1
                        )
                        self.dimlevel = 0
                    else:
                        self.state = state
                        self.dimlevel = dimlevel
                        if dimlevel > 0:
                            self.saved_dimlevel = dimlevel
                else:
                    logging.error('Dim level must be in range(0, 100).')
            else:
                # If the back-end has a dim level, but none if provided by
                # status, switch the back-end only regarding to the status
                # state.
                if self.state is False and state is True:
                    self.state = True
                    self.dimlevel = self.saved_dimlevel
                elif self.state is True and state is False:
                    self.state = False
                    self.saved_dimlevel = self.dimlevel if self.dimlevel > 0 else 1
                    self.dimlevel = 0
        else:
            self.state = state

        if self.has_temperature and 'temperature' in status.keys():
            temperature = status['temperature']
            if temperature:
                self.temperature = temperature

        if self.has_color and 'color' in status.keys():
            color = status['color']
            if color:
                self.color = color

        self.on_change(self.uid)
