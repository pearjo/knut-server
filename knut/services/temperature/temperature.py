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
import os
import pathlib
import pickle
import time


class Temperature(Events):
    # location where temperature history data are stored
    DATA_DIR = str(pathlib.Path.home()) + '/.local/share/knut/'

    def __init__(self, location, id, **kwargs):
        self.location = location
        """The location where the temperature is measured."""

        self.id = id
        """The identifier of the temperature service."""

        self.data_file = Temperature.DATA_DIR + id
        # Use -273.15 °C since this is the absolute zero temperature. If the
        # temperature is not above that value, it is invalid.

        self.temperature = -273.15  # TODO: use Kelvin instead of degree Celsius
        """Temperature in degree Celsius."""

        self.condition = str()  # TODO: move to front-end app and supply state
        """A string with the code point for the `Weather Icon
        <https://erikflowers.github.io/weather-icons/>`_ font which represents
        the current weather condition.
        """

        self.history = [list(), list()]
        """A nested list where ``history[0]`` is a list of the :attr:`temperature`
        values and ``history[1]`` the UNIX timestamps.
        """

        self.make_user_dir()
        self.load_data()

        self.__events__ = ('on_change')

    def load_data(self):
        """Load temperature history from file.

        If history data of the back-end are found, the temperature
        :attr:`history` will be set from the file.
        """
        try:
            with open(self.data_file, 'rb') as f:
                logging.info('Load temperature history for \'%s\'...'
                             % self.id)
                # read the data file as binary data stream
                self.history = pickle.load(f)
                self.check_history()
        except FileNotFoundError:
            logging.warning('Failed to load temperature history for \'%s\'.'
                            % self.id)

    def save_data(self):
        """Appends the current temperature to the :attr:`history` and writes the
        pickled representation of :attr:`history` to a file.
        """
        # check if temperature is valid before adding it to the history
        # -273.15 is the lowest possible temperature
        if self.temperature <= -273.15:
            return

        self.check_history()
        self.history[0].append(self.temperature)
        self.history[1].append(time.time())

        with open(self.data_file, 'wb') as f:
            # store the data as binary data stream
            logging.debug('Write temperature history of \'%s\' to file...' %
                          self.id)
            pickle.dump(self.history, f)

    def check_history(self):
        """Checks if the :attr:`history` is from the current day and clears the
        :attr:`history` if not.
        """
        # check first if data are in history
        if len(self.history[1]) < 1:
            return

        day_today = time.localtime().tm_mday
        day_history = time.localtime(self.history[1][-1]).tm_mday

        if (day_today > day_history):
            logging.info('Clearing temperature history of \'%s\'...'
                         % self.id)
            self.history = [list(), list()]

    def make_user_dir(self):
        """Makes a user data directory if it does not exists."""
        try:
            os.makedirs(Temperature.DATA_DIR)
        except FileExistsError:
            pass
