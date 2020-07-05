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
from .knutapi import KnutAPI
from typing import Tuple
import knut.services.temperature
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

Msg = Tuple[int, dict]


class Temperature(KnutAPI):
    """This class allows interaction with the temperature service. The following
    message types are handled by the ``request_handler()`` method of the
    superclass:

    - :const:`STATUS_REQUEST`
    - :const:`TEMPERATURE_HISTORY_REQUEST`
    - :const:`TEMPERATURE_LIST_REQUEST`

    The temperature back-ends accessed by this class are registered in the
    :attr:`backends` dictionary. They can be add using the :meth:`add_backend()`
    which also connects the back-end's ``on_change()`` event to the
    :meth:`notifier()` method.
    """
    STATUS_REQUEST = 0x0001
    STATUS_RESPONSE = 0x0101
    TEMPERATURE_LIST_REQUEST = 0x0002
    TEMPERATURE_LIST_RESPONSE = 0x0102
    TEMPERATURE_HISTORY_REQUEST = 0x0003
    TEMPERATURE_HISTORY_RESPONSE = 0x0103

    apiId = 0x01

    def __init__(self):
        super(Temperature, self).__init__()

        self.supported = {
            Temperature.STATUS_REQUEST: self.__status_request,
            Temperature.TEMPERATURE_HISTORY_REQUEST: self.__history_request,
            Temperature.TEMPERATURE_LIST_REQUEST: self.__list_request
        }

        self.backends = dict()
        """A dictionary with all back-ends where the keys are the unique names
        and the values are the corresponding temperature objects
        :class:`knut.services.temperature.Temperature`.
        """

        self.unit = UNIT

    def add_backend(self, backend: knut.services.temperature.Temperature) -> None:
        """Adds the *backend* to the :attr:`backends` dictionary."""
        if not all([hasattr(backend, 'temperature'),
                    hasattr(backend, 'condition'),
                    hasattr(backend, 'location'),
                    hasattr(backend, 'id')]):
            raise AttributeError('Data backend is missing either'
                                 + ' \'temperature\', \'condition\','
                                 + ' \'location\' or an \'id\'.')

        if backend.id in self.backends.keys():
            logging.warning('Unique name \'%s\' is not unique and backend is'
                            ' not add.' % backend.id)
            return

        self.backends[backend.id] = backend

        # add the notifier method to the back-end's on_change event
        if callable(self.backends[backend.id].on_change):
            self.backends[backend.id].on_change += self.notifier

    def notifier(self, id: str) -> None:
        """Pushes a :const:`STATUS_RESPONSE` for *id* via the
        ``on_push()`` event."""
        self.on_push(Temperature.apiId, Temperature.STATUS_RESPONSE,
                     {id: self.status(id)})

    def status(self, id: str) -> dict:
        """Returns a status dictionary of the back-end specified by its
        *id*.

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
            location = self.backends[id].location
            unit = self.unit
            condition = WEATHER_ICON_MAP[self.backends[id].condition]
            temperature = self.backends[id].temperature
        except KeyError as exception:
            logging.warning('Status of back-end \'%s\' is unkown. %s'
                            % (id, exception))

        return {
            'id': id,
            'location': location,
            'unit': unit,
            'condition': condition,
            'temperature': temperature
        }

    def __status_request(self, msg: dict) -> Msg:
        id = str()
        response = dict()

        try:
            id = msg['id']
        except KeyError:
            logging.error('Invalid STATUS_REQUEST received.')

        response[id] = self.status(id)

        return Temperature.STATUS_RESPONSE, response

    def __list_request(self, _msg: dict) -> Msg:
        backends = list()

        for id in self.backends.keys():
            backends.append(self.status(id))

        return Temperature.TEMPERATURE_LIST_RESPONSE, {'backends': backends}

    def __history_request(self, msg: dict) -> Msg:
        response = dict()

        try:
            id = msg['id']
            backend = self.backends[id]

            response['temperature'] = backend.history[0]
            response['time'] = backend.history[1]
            response['id'] = id
        except KeyError:
            logging.warning('Received temperature history request for unknown'
                            + ' temperature back-end.')

        return Temperature.TEMPERATURE_HISTORY_RESPONSE, response
