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
import numpy
import threading
import time
import urllib

from astroplan import download_IERS_A
from astroplan import Observer
from astropy.time import Time
from events import Events
import astropy


class Local(Events):
    """Provide local data like sunrise and sunset times.

    This service represent a *location* with it's sunrise and sunset time, as
    well as whether the sun is currently up or not. A daemon task is run to
    update the :attr:`is_daylight` attribute and push a ``on_change()``
    notification to listening objects.
    """

    def __init__(self, location=None, uid='local',
                 latitude=0, longitude=0, elevation=0):
        """Calculate the location data for the *location* based on the *latitude*,
        *longitude* and *elevation*. The latitude and longitude are in degree
        and the elevation in metres.
        """

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

        self.uid = uid
        """The identifier of the location service."""

        self.__daylight_timer = None  # used to update is_daylight

        self.update_observer()

        self.__events__ = ('on_change')

    def local(self):
        """Return the :class:`Local` object as dictionary.

        The dictionary has the keys ``'isDaylight'``, ``'location'``,
        ``'sunrise'``, ``'sunset'`` and ``'id'``. For example::

           {
               'isDaylight': True,
               'location': 'Hamburg',
               'sunrise': 1589513114.5880833,
               'sunset': 1589483203.418921,
               'id': 'myLocation'
           }

        """
        return {'isDaylight': bool(self.is_daylight),  # make sure that no numpy.bool_ is used
                'location': self.location,
                'sunrise': self.sunrise,
                'sunset': self.sunset,
                'id': self.uid}

    def update_observer(self):
        """Update the :attr:`observer` and :attr:`sunset` time.

        After changing any of the :attr:`longitude`, :attr:`latitude` or
        :attr:`elevation`, the :attr:`observer` should be updated using this
        method. This also updates the :attr:`sunset` time.
        """
        logging.debug('Update observer for \'%s\'...' % self.uid)
        self.observer = Observer(longitude=self.longitude*astropy.units.deg,
                                 latitude=self.latitude*astropy.units.deg,
                                 elevation=self.elevation*astropy.units.m)
        self.__get_sun_rise_and_set()
        self.__set_daylight_timer()
        self.__update_daylight()

    def __get_sun_rise_and_set(self):
        """Get the next sun rise and set time."""
        # Try to get for now and in one hour the next sun rise and set. In
        # spring the next sun set can takes more than on day when trying to get
        # the next sun set when currently the sun is setting. This would lead to
        # an NaN value and therefore the second check in one hour needs to be
        # done.
        astro_time = Time([time.time(), time.time() + 3600], format='unix')
        sun_rise_time = self.observer.sun_rise_time(astro_time, 'next')
        sun_set_time = self.observer.sun_set_time(astro_time, 'next')

        self.sunrise = sun_rise_time.min().unix
        self.sunset = sun_set_time.min().unix

        logging.debug('Next sun rise is at %f and the next sun set at %f...'
                      % (self.sunrise, self.sunset))

    def __set_daylight_timer(self):
        """Set a timer to update :attr:`is_daylight`.

        The timer resets itself as soon as it is triggered and updates the
        :attr:`is_daylight`.
        """
        def update_alarm():
            logging.debug('Update alarm for \'%s\' triggered...'
                          % self.uid)
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

        logging.debug('Set a timer for the next sun rise or set at \'%s\' which '
                      'is due in %i seconds...'
                      % (self.uid, time_from_now))

        self.__daylight_timer = threading.Timer(time_from_now, update_alarm)
        self.__daylight_timer.daemon = True
        self.__daylight_timer.start()

    def __update_daylight(self):
        """Update the :attr:`is_daylight` attribute."""
        # if the next sun set is before the next sun rise and the current time
        # is before the sun set, it must be day light at the moment
        is_daylight = self.sunset < self.sunrise and time.time() < self.sunset

        if is_daylight != self.is_daylight:
            logging.info('Day light changed for location \'%s\''
                          % self.location)
            self.is_daylight = is_daylight
            self.on_change(self.local())
