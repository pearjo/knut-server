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
import threading
import time

import requests

from .temperature import Temperature

OWM_TO_WEATHER_ICON = {
    '01d': 'day-sunny',
    '02d': 'day-cloudy',
    '03d': 'cloud',
    '04d': 'cloudy',
    '09d': 'day-showers',
    '10d': 'day-rain',
    '11d': 'day-thunderstorm',
    '13d': 'day-snow',
    '50d': 'day-fog',
    '01n': 'night-clear',
    '02n': 'night-cloudy',
    '03n': 'cloud',
    '04n': 'cloudy',
    '09n': 'night-showers',
    '10n': 'night-rain',
    '11n': 'night-thunderstorm',
    '13n': 'night-snow',
    '50n': 'night-fog',
}


class OpenWeatherMap(Temperature):
    """OpenWeatherMap temperature service.

    Extend the :class:`Temperature` base class to provide a temperature service
    which connects to `OpenWeatherMap's <https://openweathermap.org/api>`_ API.
    """

    def __init__(self, location: str, uid: str, appid: str) -> None:
        """Connect to OpenWeatherMap's API to get the weather information for
        the *location*. The unique id *uid* is used within Knut. To connect to
        the API, an *appid* is required. For more information regarding the
        appid read `OpenWeatherMap's guide
        <https://openweathermap.org/appid#get>`_.
        """
        super(OpenWeatherMap, self).__init__(location, uid)
        self.data = dict()  # stores data received from OpenWeatherMap
        self.url = str('http://api.openweathermap.org/data/2.5/weather?'
                       + 'q=' + location
                       + '&APPID=' + appid)

        # start the data poller daemon
        daemon_thread = threading.Thread(target=self.daemon,
                                         name='owm-daemon')
        daemon_thread.daemon = True
        daemon_thread.start()

        # start the data logger
        data_logger_thread = threading.Thread(target=self.data_logger,
                                              name='owm-logger')
        data_logger_thread.daemon = True
        data_logger_thread.start()

    def request_data(self):
        """Send a HTTP request to the OpenWeatherMap API."""
        try:
            self.data = requests.get(self.url).json()
            self.location = self.data['name']
            if self.data:
                self.temperature = self.data['main']['temp']
                self.condition = OWM_TO_WEATHER_ICON[self.data['weather'][0]['icon']]
        except:
            logging.warning('Can not connect to api.openweathermap.org.')

    def daemon(self, s: int=60) -> None:
        """Request every *s* seconds new weather data.

        If the data changed, the :meth:`on_change` event is called.
        """
        while True:
            previus_data = self.data
            self.request_data()

            # push data only if changed
            if previus_data != self.data:
                self.on_change(self.uid)

            time.sleep(s)

    def data_logger(self):
        """Save every hour the temperature history."""
        # wait for first value to be written to history
        while len(self.history[0]) == 0:
            self.save_data()
            time.sleep(1)

        while True:
            time.sleep(3600)
            self.save_data()
