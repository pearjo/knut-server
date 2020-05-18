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
from astroplan import Observer
from astropy.time import Time
from events import Events
import astropy
import logging
import numpy
import threading
import time


class Local(Events):
    """A Knut local service.

    This service represents a *location* and can mainly be used to get the sun
    rise and set. Furthermore, it runs a daemon task which updates the
    :attr:`is_daylight` attribute and pushes a notification :meth:`on_change()`
    to listening objects whether the sun has set or not.

    The sun's position is calculated according to the *latitude*, *longitude*
    and *elevation* of the *location*.
    """

    def __init__(self, location, unique_name, latitude=0, longitude=0,
                 elevation=0):
        self.elevation = elevation
        """The elevation of the location in meters."""

        self.is_daylight = bool()
        """Whether it is day or not."""

        self.latitude = latitude
        """The latitude of the location in degree [-90, 90]"""

        self.location = location
        """The name of the location."""

        self.longitude = longitude
        """The longitude of the location in degree [-180, 180]"""

        self.observer = None
        """The :class:`astroplan.Observer` for this location."""

        self.sunrise = float()
        """The time when the sun rises in seconds since the epoch January 1,
        1970, 00:00:00 (UTC)."""

        self.sunset = float()
        """The time when the sun sets in seconds since the epoch January 1,
        1970, 00:00:00 (UTC)."""

        self.unique_name = unique_name
        """The unique name of the location service."""

        self.__daylight_timer = None  # used to update the is_daylight attribute

        self.update_observer()
        self.__events__ = ('on_change')

    def local(self):
        """Returns the :class:`Local` object as dictionary.

        The returned dictionary has the keys ``'isDaylight'``, ``'location'``,
        ``'sunrise'``, ``'sunset'`` and ``'uniqueName'``. For example::

           {
               'isDaylight': True,
               'location': 'Hamburg',
               'sunrise': 1589513114.5880833,
               'sunset': 1589483203.418921,
               'uniqueName': 'myLocation'
           }

        """
        return {'isDaylight': self.is_daylight,
                'location': self.location,
                'sunrise': self.sunrise,
                'sunset': self.sunset,
                'uniqueName': self.unique_name}

    def update_observer(self):
        """Updates the :attr:`observer` and :attr:`sunset` time.

        After changing any of the :attr:`longitude`, :attr:`latitude` or
        :attr:`elevation`, the :attr:`observer` should be updated using this
        method. This also updates the :attr:`sunset` time.
        """
        logging.debug('Update observer for \'%s\'...' % self.unique_name)
        self.observer = Observer(longitude=self.longitude*astropy.units.deg,
                                 latitude=self.latitude*astropy.units.deg,
                                 elevation=self.elevation*astropy.units.m)
        self.__get_sun_rise_and_set()
        self.__set_daylight_timer()

    def __get_sun_rise_and_set(self):
        """Gets the next sun rise and set time."""
        astro_time = Time(time.time(), format='unix')
        sun_rise_time = self.observer.sun_rise_time(astro_time, 'next')
        sun_set_time = self.observer.sun_set_time(astro_time, 'next')

        self.sunrise = sun_rise_time.unix
        self.sunset = sun_set_time.unix

        logging.debug('Next sun rise is at %f and the next sun set at %f...'
                      % (self.sunrise, self.sunset))

    def __set_daylight_timer(self):
        """Sets a timer to update :attr:`is_daylight`.

        The timer resets itself as soon as it is triggered and updates the
        :attr:`is_daylight`.
        """
        def update_alarm():
            logging.debug('Update alarm for \'%s\' triggered...'
                          % self.unique_name)
            self.__get_sun_rise_and_set()
            self.__update_daylight()
            self.__set_daylight_timer()

        if self.__daylight_timer:
            self.__daylight_timer.cancel()

        # the time in seconds until the next sun rise or set
        if self.sunset < self.sunrise and time.time() < self.sunset:
            # we have currently daylight and set the timer to the sun set
            time_from_now = int(round(self.sunset - time.time(), 0))
        else:
            time_from_now = int(round(self.sunrise - time.time(), 0))

        logging.debug('Set a timer for the next sun rise or set at \'%s\' which ' \
                      'is due in %i seconds...'
                      % (self.unique_name, time_from_now))

        self.__daylight_timer = threading.Timer(time_from_now, update_alarm)
        self.__daylight_timer.start()

    def __update_daylight(self):
        """Updates the :attr:`is_daylight` attribute."""
        if (self.sunset or self.sunrise) is numpy.nan:
            logging.warning("Invalid sun set or rise...")
            return

        # if the next sun set is before the next sun rise and the current time
        # is before the sun set, it must be day light at the moment
        if self.sunset < self.sunrise and time.time() < self.sunset:
            is_daylight = True
        else:
            is_daylight = False

        if is_daylight != self.is_daylight:
            logging.debug('Day light changed for location \'%s\''
                          % self.location)
            self.is_daylight = is_daylight
            self.on_change(self.local())
