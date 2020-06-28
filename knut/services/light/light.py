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

    def __init__(self, location, unique_name, room):
        """The light has a *unique_name* and has a *location* within a *room*.

        If the state of the light changes, e.g. when calling
        :meth:`status_setter()`, the method :meth:`on_change()` is called with
        the *unique_name* as argument. Any listener can register upon this
        event.

        """
        self.has_color = bool()
        self.has_dimlevel = bool()
        self.has_temperature = bool()

        self.location = location
        """The location inside a :attr:`room`."""
        self.unique_name = unique_name
        """The unique name used by the Knut system."""
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

        # Call on_change as method with the unique_name as argument to notify
        # listening methods.
        self.__events__ = ('on_change')

    def status_setter(self, status):
        """Applies the *status* to the back-end.

        The *status* dictionary must have ``state`` as key and can have the
        following keys additional:

        * ``dimlevel``
        * ``temperature``
        * ``color``

        """
        state = status['state']
        if not self.has_dimlevel:
            self.state = state

        # If the back-end state is false and either the status state is true or
        # the status dim level is > 0, switch back-end state to true and set the
        # last active dim level. Else, if the back-end state is true and either
        # the status dim level is 0 or state is false, switch the back-end state
        # to false and the dim level to 0. If none of the two cases are given,
        # the state and dim level of the back-end are set from the status.
        if self.has_dimlevel and 'dimlevel' in status.keys():
            dimlevel = status['dimlevel']

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
                logging.error(('Tried to set dim level of %.2f for \'%s\'.'
                               + ' Dim level must be in range(0, 100).')
                              % (dimlevel, self.unique_name))
        # If the back-end has a dim level, but none if provided by status,
        # switch the back-end only regarding to the status state.
        elif self.has_dimlevel and 'dimlevel' not in status.keys():
            if self.state is False and state is True:
                self.state = True
                self.dimlevel = self.saved_dimlevel
            elif self.state is True and state is False:
                self.state = False
                self.saved_dimlevel = self.dimlevel if self.dimlevel > 0 else 1
                self.dimlevel = 0

        if self.has_temperature and 'temperature' in status.keys():
            temperature = status['temperature']
            self.temperature = temperature

        if self.has_color and 'color' in status.keys():
            color = status['color']
            self.color = color

        self.on_change(self.unique_name)
