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
from events import Events
import threading
import time


class DummyTemperature(Events):
    def __init__(self, location, unique_name):
        self._temperature = 10.2
        self.unique_name = unique_name
        self.location = location
        """Represents the temperature history of one day. history[0] holds
        a list of the temperature values and history[1] the time stamps in the
        format hh:mm:ss."""
        self.history = [list(), list()]
        self.__events__ = ('on_change')

        # start tracker
        trackerThread = threading.Thread(target=self.tracker)
        trackerThread.daemon = True
        trackerThread.start()

    def tracker(self):
        while True:
            # clear history after day changed
            try:
                if (time.strptime(self.history[1][-1], "%H:%M:%S")
                        > time.localtime()):
                    self.history = [list(), list()]
            except IndexError:
                pass

            self.history[0].append(self.temperature)
            self.history[1].append(time.strftime("%H:%M:%S"))
            time.sleep(60)

    @property
    def temperature(self):
        return self._temperature

    @property
    def condition(self):
        return 'day-cloudy'
