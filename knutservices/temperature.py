from events import Events
import logging
import os
import pathlib
import pickle
import time

# location to save temperature history data
DATA_DIR = str(pathlib.Path.home()) + '/.local/share/knut/'


class Temperature(Events):
    def __init__(self, location, unique_name, **kwargs):
        self.location = location
        self.unique_name = unique_name
        self.data_file = DATA_DIR + unique_name
        self.temperature = float()  # TODO: use Kelvin instead of degree Celsius
        """Temperature in degree Celsius."""
        self.condition = str()  # TODO: move to front-end app and supply state
        """A string with the code point for the `Weather Icon
        <https://erikflowers.github.io/weather-icons/>`_ font which represents
        the current weather condition.
        """
        self.history = [list(), list()]
        """A nested list where ``history[0]`` is a list of the :attr:`temperature`
        values and ``history[1]`` the time stamps in seconds since the epoch
        January 1, 1970, 00:00:00 (UTC).
        """
        self.make_user_dir()
        self.load_data()

        self.__events__ = ('on_change')

    def __del__(self):
        self.save_data()

    def load_data(self):
        """Load temperature history from file.

        If history data of the back-end are found, the temperature
        :attr:`history` will be set from the file.
        """
        try:
            with open(self.data_file, 'rb') as f:
                # read the data file as binary data stream
                self.history = pickle.load(f)
        except FileNotFoundError:
            pass

    def save_data(self):
        """Appends the current temperature to the :attr:`history` and writes the
        pickled representation of :attr:`history` to a file.
        """
        current_time = time.time()

        try:
            if (time.localtime(current_time).tm_mday
                    > time.localtime(self.history[1][-1]).tm_mday):
                logging.info('Clearing temperature history of \'%s\'...'
                             % self.unique_name)
                self.history = [list(), list()]
        except IndexError:
            pass

        self.history[0].append(self.temperature)
        self.history[1].append(current_time)

        with open(self.data_file, 'wb') as f:
            # store the data as binary data stream
            logging.debug('Write temperature history of \'%s\' to file...' %
                          self.unique_name)
            pickle.dump(self.history, f)

    def make_user_dir(self):
        """Makes a user data directory if it does not exists."""
        try:
            os.makedirs(DATA_DIR)
        except FileExistsError:
            pass
