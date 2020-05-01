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
import knut.services
import logging

UNIT = '°C'
WEATHER_ICON_MAP = {
    None: '',
    'alien': '\uf075',
    'barometer': '\uf079',
    'celsius': '\uf03c',
    'cloud': '\uf041',
    'cloud-down': '\uf03d',
    'cloud-refresh': '\uf03e',
    'cloud-up': '\uf040',
    'cloudy': '\uf013',
    'cloudy-gusts': '\uf011',
    'cloudy-windy': '\uf012',
    'day-cloudy': '\uf002',
    'day-cloudy-gusts': '\uf000',
    'day-cloudy-high': '\uf07d',
    'day-cloudy-windy': '\uf001',
    'day-fog': '\uf003',
    'day-hail': '\uf004',
    'day-haze': '\uf0b6',
    'day-light-wind': '\uf0c4',
    'day-lightning': '\uf005',
    'day-rain': '\uf008',
    'day-rain-mix': '\uf006',
    'day-rain-wind': '\uf007',
    'day-showers': '\uf009',
    'day-sleet': '\uf0b2',
    'day-sleet-storm': '\uf068',
    'day-snow': '\uf00a',
    'day-snow-thunderstorm': '\uf06b',
    'day-snow-wind': '\uf065',
    'day-sprinkle': '\uf00b',
    'day-storm-showers': '\uf00e',
    'day-sunny': '\uf00d',
    'day-sunny-overcast': '\uf00c',
    'day-thunderstorm': '\uf010',
    'day-windy': '\uf085',
    'degrees': '\uf042',
    'direction-down': '\uf044',
    'direction-down-left': '\uf043',
    'direction-down-right': '\uf088',
    'direction-left': '\uf048',
    'direction-right': '\uf04d',
    'direction-up': '\uf058',
    'direction-up-left': '\uf087',
    'direction-up-right': '\uf057',
    'dust': '\uf063',
    'earthquake': '\uf0c6',
    'fahrenheit': '\uf045',
    'fire': '\uf0c7',
    'flood': '\uf07c',
    'fog': '\uf014',
    'forecast-io-clear-day': '\uf00d',
    'forecast-io-clear-night': '\uf02e',
    'forecast-io-cloudy': '\uf013',
    'forecast-io-fog': '\uf014',
    'forecast-io-hail': '\uf015',
    'forecast-io-partly-cloudy-day': '\uf002',
    'forecast-io-partly-cloudy-night': '\uf031',
    'forecast-io-rain': '\uf019',
    'forecast-io-sleet': '\uf0b5',
    'forecast-io-snow': '\uf01b',
    'forecast-io-thunderstorm': '\uf01e',
    'forecast-io-tornado': '\uf056',
    'forecast-io-wind': '\uf050',
    'gale-warning': '\uf0cd',
    'hail': '\uf015',
    'horizon': '\uf047',
    'horizon-alt': '\uf046',
    'hot': '\uf072',
    'humidity': '\uf07a',
    'hurricane': '\uf073',
    'hurricane-warning': '\uf0cf',
    'lightning': '\uf016',
    'lunar-eclipse': '\uf070',
    'meteor': '\uf071',
    'moon-0': '\uf095',
    'moon-1': '\uf096',
    'moon-10': '\uf09f',
    'moon-11': '\uf0a0',
    'moon-12': '\uf0a1',
    'moon-13': '\uf0a2',
    'moon-14': '\uf0a3',
    'moon-15': '\uf0a4',
    'moon-16': '\uf0a5',
    'moon-17': '\uf0a6',
    'moon-18': '\uf0a7',
    'moon-19': '\uf0a8',
    'moon-2': '\uf097',
    'moon-20': '\uf0a9',
    'moon-21': '\uf0aa',
    'moon-22': '\uf0ab',
    'moon-23': '\uf0ac',
    'moon-24': '\uf0ad',
    'moon-25': '\uf0ae',
    'moon-26': '\uf0af',
    'moon-27': '\uf0b0',
    'moon-3': '\uf098',
    'moon-4': '\uf099',
    'moon-5': '\uf09a',
    'moon-6': '\uf09b',
    'moon-7': '\uf09c',
    'moon-8': '\uf09d',
    'moon-9': '\uf09e',
    'moon-alt-first-quarter': '\uf0d6',
    'moon-alt-full': '\uf0dd',
    'moon-alt-new': '\uf0eb',
    'moon-alt-third-quarter': '\uf0e4',
    'moon-alt-waning-crescent-1': '\uf0e5',
    'moon-alt-waning-crescent-2': '\uf0e6',
    'moon-alt-waning-crescent-3': '\uf0e7',
    'moon-alt-waning-crescent-4': '\uf0e8',
    'moon-alt-waning-crescent-5': '\uf0e9',
    'moon-alt-waning-crescent-6': '\uf0ea',
    'moon-alt-waning-gibbous-1': '\uf0de',
    'moon-alt-waning-gibbous-2': '\uf0df',
    'moon-alt-waning-gibbous-3': '\uf0e0',
    'moon-alt-waning-gibbous-4': '\uf0e1',
    'moon-alt-waning-gibbous-5': '\uf0e2',
    'moon-alt-waning-gibbous-6': '\uf0e3',
    'moon-alt-waxing-crescent-1': '\uf0d0',
    'moon-alt-waxing-crescent-2': '\uf0d1',
    'moon-alt-waxing-crescent-3': '\uf0d2',
    'moon-alt-waxing-crescent-4': '\uf0d3',
    'moon-alt-waxing-crescent-5': '\uf0d4',
    'moon-alt-waxing-crescent-6': '\uf0d5',
    'moon-alt-waxing-gibbous-1': '\uf0d7',
    'moon-alt-waxing-gibbous-2': '\uf0d8',
    'moon-alt-waxing-gibbous-3': '\uf0d9',
    'moon-alt-waxing-gibbous-4': '\uf0da',
    'moon-alt-waxing-gibbous-5': '\uf0db',
    'moon-alt-waxing-gibbous-6': '\uf0dc',
    'moon-first-quarter': '\uf09c',
    'moon-full': '\uf0a3',
    'moon-new': '\uf095',
    'moon-third-quarter': '\uf0aa',
    'moon-waning-crescent-1': '\uf0ab',
    'moon-waning-crescent-2': '\uf0ac',
    'moon-waning-crescent-3': '\uf0ad',
    'moon-waning-crescent-4': '\uf0ae',
    'moon-waning-crescent-5': '\uf0af',
    'moon-waning-crescent-6': '\uf0b0',
    'moon-waning-gibbous-1': '\uf0a4',
    'moon-waning-gibbous-2': '\uf0a5',
    'moon-waning-gibbous-3': '\uf0a6',
    'moon-waning-gibbous-4': '\uf0a7',
    'moon-waning-gibbous-5': '\uf0a8',
    'moon-waning-gibbous-6': '\uf0a9',
    'moon-waxing-crescent-1': '\uf096',
    'moon-waxing-crescent-2': '\uf097',
    'moon-waxing-crescent-3': '\uf098',
    'moon-waxing-crescent-4': '\uf099',
    'moon-waxing-crescent-5': '\uf09a',
    'moon-waxing-crescent-6': '\uf09b',
    'moon-waxing-gibbous-1': '\uf09d',
    'moon-waxing-gibbous-2': '\uf09e',
    'moon-waxing-gibbous-3': '\uf09f',
    'moon-waxing-gibbous-4': '\uf0a0',
    'moon-waxing-gibbous-5': '\uf0a1',
    'moon-waxing-gibbous-6': '\uf0a2',
    'moonrise': '\uf0c9',
    'moonset': '\uf0ca',
    'na': '\uf07b',
    'night-alt-cloudy': '\uf086',
    'night-alt-cloudy-gusts': '\uf022',
    'night-alt-cloudy-high': '\uf07e',
    'night-alt-cloudy-windy': '\uf023',
    'night-alt-hail': '\uf024',
    'night-alt-lightning': '\uf025',
    'night-alt-partly-cloudy': '\uf081',
    'night-alt-rain': '\uf028',
    'night-alt-rain-mix': '\uf026',
    'night-alt-rain-wind': '\uf027',
    'night-alt-showers': '\uf029',
    'night-alt-sleet': '\uf0b4',
    'night-alt-sleet-storm': '\uf06a',
    'night-alt-snow': '\uf02a',
    'night-alt-snow-thunderstorm': '\uf06d',
    'night-alt-snow-wind': '\uf067',
    'night-alt-sprinkle': '\uf02b',
    'night-alt-storm-showers': '\uf02c',
    'night-alt-thunderstorm': '\uf02d',
    'night-clear': '\uf02e',
    'night-cloudy': '\uf031',
    'night-cloudy-gusts': '\uf02f',
    'night-cloudy-high': '\uf080',
    'night-cloudy-windy': '\uf030',
    'night-fog': '\uf04a',
    'night-hail': '\uf032',
    'night-lightning': '\uf033',
    'night-partly-cloudy': '\uf083',
    'night-rain': '\uf036',
    'night-rain-mix': '\uf034',
    'night-rain-wind': '\uf035',
    'night-showers': '\uf037',
    'night-sleet': '\uf0b3',
    'night-sleet-storm': '\uf069',
    'night-snow': '\uf038',
    'night-snow-thunderstorm': '\uf06c',
    'night-snow-wind': '\uf066',
    'night-sprinkle': '\uf039',
    'night-storm-showers': '\uf03a',
    'night-thunderstorm': '\uf03b'
}


class Temperature(Events):
    """Knut temperature API.

    The temperature object has a dictionary :attr:`backends` of all available
    back-ends. A back-end can be added using :meth:`add_backend()`.  To interact
    with the back-ends, this API has as request handler method
    :meth:`request_handler()`. Using this method, requests can be send to the
    back-ends to get e.g. the current weather data.

    The following message types *msg_id* are supported with the required
    message *msg* and their *response* with its *response_id*:

    .. py:data:: STATUS_REQUEST
       :value: 0x0001

       Requests the status of a back-end. The message *msg* must have the
       key ``'uniqueName'``. For example::

          {"uniqueName": "myTemperatureBackend"}

    .. py:data:: STATUS_RESPONSE
       :value: 0x0101

       The status *response* has as key the
       :attr:`knut.services.Temperature.unique_name` of the specific back-end
       and as value the dictionary returned by :meth:`status()`. For example::

          {
              "myTemperatureBackend": {
                  "location": "Miami",
                  "unit": "°C",
                  "condition": "\\uf00d",
                  "temperature": 30.1
              }
          }

    .. py:data:: TEMPERATURE_LIST_REQUEST
       :value: 0x0002

       Requests a list of all temperature back-ends with their status. The
       message *msg* for this request can be an empty dictionary.

    .. py:data:: TEMPERATURE_LIST_RESPONSE
       :value: 0x0102

       The *response* of the temperature list response is similar to
       the :const:`STATUS_RESPONSE`, only with all known back-ends that are in
       :attr:`backends`. For example::


          {
              "myTemperatureBackend1": {
                  "location": "Miami",
                  "unit": "°C",
                  "condition": "\\uf00d",
                  "temperature": 30.420000000000009
              },
              "myTemperatureBackend2": {
                  "location": "Hamburg",
                  "unit": "°C",
                  "condition": "\\uf008",
                  "temperature": 14.240000000000009
              }
          }

    .. py:data:: TEMPERATURE_HISTORY_REQUEST
       :value: 0x0003

       Request the temperature history of a back-end. The *msg* must be in
       the same format as for a :const:`STATUS_REQUEST`.

    .. py:data:: TEMPERATURE_HISTORY_RESPONSE
       :value: 0x0103

       The *response* is a dictionary with the keys ``'uniqueName'``, ``'time'``
       and ``'temperature'``. The *time* value is an array of floats with the
       time in seconds since the epoch January 1, 1970, 00:00:00 (UTC). The
       *temperature* value is also an array with the corresponding temperature
       values as float. For example::

          {
              "uniqueName": "myTemperatureBackend",
              "temperature": [
                  30.420000000000009,
                  32.420000000000009
              ],
              "time": [
                  1581863822.2132704,
                  1581863882.2132704
              ]
          }

    """
    NULL = 0x0000
    STATUS_REQUEST = 0x0001
    STATUS_RESPONSE = 0x0101
    TEMPERATURE_LIST_REQUEST = 0x0002
    TEMPERATURE_LIST_RESPONSE = 0x0102
    TEMPERATURE_HISTORY_REQUEST = 0x0003
    TEMPERATURE_HISTORY_RESPONSE = 0x0103

    serviceid = 0x01
    """The temperature service id."""

    def __init__(self):
        self.backends = dict()
        """A dictionary with all back-ends where the keys are the
        :attr:`knut.services.Temperature.unique_name` and the values are the
        corresponding temperature objects :class:`knut.services.Temperature`
        """
        self.unit = UNIT
        self.__events__ = ('on_push')

    def add_backend(self, backend):
        """Adds the *backend* to :attr:`backends`."""
        if not all([hasattr(backend, 'temperature'),
                    hasattr(backend, 'condition'),
                    hasattr(backend, 'location'),
                    hasattr(backend, 'unique_name')]):
            raise AttributeError('Data backend is missing either'
                                 + ' \'temperature\', \'condition\','
                                 + ' \'location\' or an \'unique_name\'.')

        if backend.unique_name in self.backends.keys():
            logging.warning('Unique name \'%s\' is not unique and backend is'
                            ' not add.' % backend.unique_name)
            return

        self.backends[backend.unique_name] = backend

        # add the notifier method to the back-end's on_change event
        if callable(self.backends[backend.unique_name].on_change):
            self.backends[backend.unique_name].on_change += self.notifier

    def request_handler(self, msg_id, msg):
        """Returns the tuple (*response_id*, *response*) upon a request.

        The following messages *msg* with their *msg_id* can be send by a client
        and will be handled:

        - :const:`STATUS_REQUEST`
        - :const:`TEMPERATURE_LIST_REQUEST`
        - :const:`TEMPERATURE_HISTORY_REQUEST`
        """
        response = dict()
        response_id = Temperature.NULL

        logging.debug('Received temperature request.')

        if msg_id == Temperature.STATUS_REQUEST:
            response_id, response = self._handle_status_request(msg)
        elif msg_id == Temperature.TEMPERATURE_LIST_REQUEST:
            response_id, response = self._handle_temperature_list_request()
        elif msg_id == Temperature.TEMPERATURE_HISTORY_REQUEST:
            response_id, response = self._handle_temperature_history_request(
                msg
            )

        # check if the response is valid
        response_id = response_id if len(response) > 0 else Temperature.NULL
        return response_id, response

    def notifier(self, unique_name):
        self.on_push(Temperature.serviceid, Temperature.STATUS_RESPONSE,
                     {unique_name: self.status(unique_name)})

    def status(self, unique_name):
        """Returns a status dictionary of the back-end specified by its
        *unique_name*.

        The returned dictionary has the keys:

        - ``'location'`` as string where the temperature is measured
        - ``'unit'`` as string with the value ``°C``
        - ``'condition'`` is a string with the code point for the `Weather Icon
          <https://erikflowers.github.io/weather-icons/>`_ font
        - ``'temperature'`` as a float
        """
        location = str()
        unit = str()
        condition = str()
        temperature = float()

        try:
            location = self.backends[unique_name].location
            unit = self.unit
            condition = WEATHER_ICON_MAP[self.backends[unique_name].condition]
            temperature = self.backends[unique_name].temperature
        except KeyError as exception:
            logging.warning('Status of back-end \'%s\' is unkown. %s'
                            % (unique_name, exception))

        return {
            'location': location,
            'unit': unit,
            'condition': condition,
            'temperature': temperature
        }

    def _handle_status_request(self, msg):
        """Handles a ``STATUS_REQUEST`` and returns the tuple (STATUS_RESPONSE,
        response).
        """
        unique_name = str()
        response = dict()

        try:
            unique_name = msg['uniqueName']
        except KeyError:
            logging.error('Invalid STATUS_REQUEST received.')

        response[unique_name] = self.status(unique_name)

        return Temperature.STATUS_RESPONSE, response

    def _handle_temperature_list_request(self):
        """Handles a ``TEMPERATURE_LIST_REQUEST`` and returns a tuple
        (TEMPERATURE_HISTORY_RESPONSE, response).
        """
        response = dict()

        for temperature in self.backends.keys():
            response[temperature] = self.status(temperature)

        return Temperature.TEMPERATURE_LIST_RESPONSE, response

    def _handle_temperature_history_request(self, msg):
        """Handles a ``TEMPERATURE_HISTORY_REQUEST`` and returns a tuple
        (TEMPERATURE_HISTORY_RESPONSE, response).
        """
        response = dict()

        try:
            unique_name = msg['uniqueName']
            backend = self.backends[unique_name]

            response['temperature'] = backend.history[0]
            response['time'] = backend.history[1]
            response['uniqueName'] = unique_name
        except KeyError:
            logging.warning('Received temperature history request for unknown'
                            + ' temperature back-end.')

        return Temperature.TEMPERATURE_HISTORY_RESPONSE, response