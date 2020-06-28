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
from .knutapi import KnutAPI
import logging


class Room(Events):
    """Bundles :class:`knut.services.Light` objects in a room.

    Using the room object, all lights within one *room* can be controlled at
    once.
    """
    serviceid = 0x02
    """The room service id. The id is the same as the light service id since
    this service is part of the light service."""

    def __init__(self, room):
        self.backends = list()
        """A dictionary with all back-ends in the room where the keys are the
        :attr:`knut.services.Light.unique_name` and the values are the
        corresponding light objects :class:`knut.services.Light`.
        """
        self.room = room
        """The room name."""
        self.state = float()
        """The state of the room where 0 is all lights are off, 1 all lights on
        and 0.5 when neither all lights are on nor off."""
        # use the on_push event to push new status to parent object
        self.__events__ = ('on_push')
        logging.debug('Added room \'%s\' to bundle lights' % self.room)

    def add_backend(self, backend):
        """Add a light back-end to the room."""
        if backend.room != self.room:
            logging.warning('Data backend \'%s\' is not in room \'%s\''
                            % (backend.unique_name, self.room))
            return

        if backend not in self.backends:
            self.backends.append(backend)
            self.fetch()

    def switch(self, state):
        """Switch all lights in :attr:`backends` on or off."""
        for light in self.backends:
            if state == 1:
                light.status_setter({'state': True})
            else:
                light.status_setter({'state': False})

        self.state = state

    def status(self):
        """Returns the room state.

        Returns a dictionary with the keys ``'room'`` and ``'state'``.

        The state is 0 if all lights are off, 1 if all lights are on and
        0.5 if neither all lights are on nor off.
        """
        return {'room': self.room,
                'state': self.state}

    def fetch(self):
        """Fetch light states of all backends.

        Fetch the light states of all backends in the room and sets the
        :attr:`state` accordingly to 0 if all off, to 1 if all on or to 0.5
        if neither all are on nor off. If the :attr:`state` changed, a
        :const:`knut.apis.Light.ROOM_RESPONSE` will be pushed.
        """
        state_set = set((light.state) for light in self.backends)

        # If all light states are equal, the set has one entry being the state.
        # Using pop() returns that entry and the light state can be set.
        if len(state_set) == 1:
            state = 1 if state_set.pop() else 0
        else:
            state = 0.5

        if self.state != state:
            self.state = state
            # push new status to registered event listeners
            self.on_push(Room.serviceid, Light.ROOM_RESPONSE, self.status())


class Light(KnutAPI):
    """This class allows interaction with the light service. The following message
    types are handled by the ``request_handler()``:

    - :const:`STATUS_REQUEST`
    - :const:`STATUS_RESPONSE`
    - :const:`LIGHTS_REQUEST`
    - :const:`ALL_LIGHTS_REQUEST`
    - :const:`ALL_LIGHTS_RESPONSE`
    - :const:`ROOMS_LIST_REQUEST`
    - :const:`ROOM_REQUEST`

    This class can interact with all service back-ends that are registered in
    the dictionary :attr:`backends`. Those are add using the
    :meth:`add_backend()` method.

    .. note::

       Each light service back-end add to the dictionary must subclass the
       :class:`~knut.services.light.Light` class.

    .. py:data:: STATUS_REQUEST
       :value: 0x0001

       Requests the status of a back-end. The message must have the key
       ``'uniqueName'`` with the unique name of the requested back-end as
       value. For example::

          {'uniqueName': 'myLightBackend'}

    .. py:data:: STATUS_RESPONSE
       :value: 0x0101

       The status of a back-end. Content of the dictionary is the return value
       of the ``status()`` method. See :meth:`status()` for details .
       Following an example response::

          {
              'uniqueName': 'myLightBackend',
              'location': 'Somewhere in a room...',
              'room': 'Some room',
              'state': False,
              'hasTemperature': True,
              'hasDimlevel': True,
              'hasColor': False,
              'temperature': 100,
              'colorCold': '#f5faf6',
              'colorWarm': '#efd275',
              'dimlevel': 50,
              'color': ''
          }

    .. py:data:: LIGHTS_REQUEST
       :value: 0x0002

       Requests the status of all back-ends. No message is required.

    .. py:data:: LIGHTS_RESPONSE
       :value: 0x0102

       This response is similar to the :const:`STATUS_RESPONSE`, only with all
       known back-ends that are in :attr:`backends`. For example::

          {
              'myLightBackend1': {
                  'location': 'Somewhere in a room...',
                  'room': 'Some room',
                  'state': False,
                  'hasTemperature': True,
                  'hasDimlevel': True,
                  'hasColor': False,
                  'temperature': 100,
                  'colorCold': '#f5faf6',
                  'colorWarm': '#efd275',
                  'dimlevel': 50,
                  'color': ''
              },
              'myLightBackend1': {
                  'location': 'Somewhere else in a room...',
                  'room': 'Some other room',
                  'state': True,
                  'hasTemperature': True,
                  'hasDimlevel': True,
                  'hasColor': False,
                  'temperature': 100,
                  'colorCold': '#f5faf6',
                  'colorWarm': '#efd275',
                  'dimlevel': 50,
                  'color': ''
              }
          }

    .. py:data:: ALL_LIGHTS_REQUEST
       :value: 0x0003

       Requests the combined state of all lights. No message is required.

    .. py:data:: ALL_LIGHTS_RESPONSE
       :value: 0x0103

       The response is a dictionary with the key ``'state'``, where the value
       *0* is all lights off and *1* all lights on. If the value is *0.5*,
       neither all lights are on nor off. If this message is incoming, the
       values *0* and *1* can only be set. For example::

          {'state': 1.0}

    .. py:data:: ROOMS_LIST_REQUEST
       :value: 0x0004

       Requests a list of all rooms with their state. No message is required.

    .. py:data:: ROOMS_LIST_RESPONSE
       :value: 0x0104

       This response is a dictionary with the unique names of the rooms as keys
       and their state as values. The possible states are analog to the
       :const:`ALL_LIGHTS_RESPONSE` values::

          {
              'myRoom1': 0.5,
              'myRoom2': 0.0
          }

    .. py:data:: ROOM_REQUEST
       :value: 0x0005

       Requests to set the state for a room. The keys ``'room'`` and ``'state'``
       are required, where only the states *0* and *1* are applied. For
       example::

          {
              'room': 'myRoom1',
              'state': 0
          }

    .. py:data:: ROOM_RESPONSE
       :value: 0x0105

       The response dictionary has the same entries as the message of the
       :const:`ROOM_REQUEST`.

    """
    STATUS_REQUEST = 0x0001
    STATUS_RESPONSE = 0x0101
    LIGHTS_REQUEST = 0x0002
    LIGHTS_RESPONSE = 0x0102
    ALL_LIGHTS_REQUEST = 0x0003
    ALL_LIGHTS_RESPONSE = 0x0103
    ROOMS_LIST_REQUEST = 0x0004
    ROOMS_LIST_RESPONSE = 0x0104
    ROOM_REQUEST = 0x0005
    ROOM_RESPONSE = 0x0105

    serviceid = 0x02
    """The light service identifier."""

    def __init__(self):
        super(Light, self).__init__()
        self.supported = {
            Light.STATUS_REQUEST: self.__handle_status_request,
            Light.STATUS_RESPONSE: self.__handle_status_response,
            Light.LIGHTS_REQUEST: self.__handle_lights_request,
            Light.ALL_LIGHTS_REQUEST: self.__handle_all_lights_request,
            Light.ALL_LIGHTS_RESPONSE: self.__handle_all_lights_response,
            Light.ROOMS_LIST_REQUEST: self.__handle_rooms_list_request,
            Light.ROOM_REQUEST: self.__handle_room_request
        }

        self.backends = dict()
        """A dictionary with all back-ends where the keys are the
        unique names of the back-ends and the values are the
        corresponding light objects :class:`knut.services.Light`.
        """
        self.rooms = dict()
        self.light_state_all = float()

    def add_backend(self, backend):
        """Adds the *backend* to the :attr:`backends` dictionary."""
        if not all([hasattr(backend, 'has_temperature'),
                    hasattr(backend, 'has_color'),
                    hasattr(backend, 'has_dimlevel'),
                    hasattr(backend, 'state'),
                    hasattr(backend, 'location'),
                    hasattr(backend, 'room'),
                    hasattr(backend, 'unique_name')]):
            raise AttributeError('Data backend is missing either \'temperature\','
                                 + ' \'color\', \'dimlevel\', \'state\','
                                 + ' \'location\' or an \'unique_name\'.')

        if backend.unique_name in self.backends.keys():
            logging.warning('Name \'%s\' is not unique, hence backend is not add.'
                            % backend.unique_name)
            return

        self.backends[backend.unique_name] = backend

        # add room to rooms dictionary
        if backend.room not in self.rooms.keys():
            self.rooms[backend.room] = Room(backend.room)
            self.rooms[backend.room].on_push += self.on_push

        self.rooms[backend.room].add_backend(backend)

        # add the notifier method to the back-end's on_change event
        if callable(self.backends[backend.unique_name].on_change):
            self.backends[backend.unique_name].on_change += self.notifier

    def fetch(self):
        state_set = set((light.state) for light in self.backends.values())

        # If all light states are equal, the set has one entry being the state.
        # Using pop() returns that entry and the light state can be set.
        if len(state_set) == 1:
            light_state_all = 1 if state_set.pop() else 0
        else:
            light_state_all = 0.5

        if self.light_state_all != light_state_all:
            self.light_state_all = light_state_all
            # push response to all clients
            self.on_push(Light.serviceid, Light.ALL_LIGHTS_RESPONSE,
                         {'state': self.light_state_all})

    def request_handler(self, msg_id, msg):
        # override of the request_handler
        response_id, response = super(Light, self).request_handler(msg_id, msg)

        self.fetch()  # update light_state_all

        # check if the response is valid
        response_id = response_id if len(response) > 0 else Light.NULL

        return response_id, response

    def notifier(self, unique_name):
        """Pushes the :meth:`status()` of the back-end with the *unique_name* to all
        listeners of the ``on_push()`` event.
        """
        # update room in which the light is located
        light = self.backends[unique_name]
        self.rooms[light.room].fetch()

        # push the message to registered objects
        logging.debug('Push status of \'%s\' to listeners.' % unique_name)
        self.on_push(Light.serviceid, Light.STATUS_RESPONSE,
                     self.status(unique_name))

    def status(self, unique_name):
        """Returns the status information of the light *unique_name*.

        The status dictionary has the following keys:

        - ``'color'``
        - ``'colorCold'``
        - ``'colorWarm'``
        - ``'dimlevel'``
        - ``'hasColor'``
        - ``'hasDimlevel'``
        - ``'hasTemperature'``
        - ``'location'``
        - ``'room'``
        - ``'state'``
        - ``'temperature'``
        - ``'uniqueName'``

        See the documentation of the :class:`knut.services.Light` class for more
        information the value of each key.

        """
        light = self.backends[unique_name]
        return {
            'uniqueName': light.unique_name,
            'location': light.location,
            'room': light.room,
            'state': light.state,
            'hasTemperature': light.has_temperature,
            'hasDimlevel': light.has_dimlevel,
            'hasColor': light.has_color,
            'temperature': light.temperature if light.has_temperature else int(),
            'colorCold': light.color_cold if light.has_temperature else str(),
            'colorWarm': light.color_warm if light.has_temperature else str(),
            'dimlevel': light.dimlevel if light.has_dimlevel else int(),
            'color': light.color if light.has_color else str()
        }

    def __handle_status_request(self, msg):
        response = dict()
        response_id = Light.NULL

        if msg['uniqueName'] not in self.backends.keys():
            logging.warning('No light service with unique name \'%s\' known.'
                            % msg['uniqueName'])
        else:
            response = self.status(msg['uniqueName'])
            response_id = Light.STATUS_RESPONSE

        return response_id, response

    def __handle_lights_request(self, _):
        response = dict()
        response_id = Light.LIGHTS_RESPONSE

        for light in self.backends.keys():
            response[light] = self.status(light)

        return response_id, response

    def __handle_status_response(self, msg):
        response = dict()
        response_id = Light.NULL

        if msg['uniqueName'] not in self.backends.keys():
            logging.warning('No light service with unique name \'%s\' known.'
                            % msg['uniqueName'])
        else:
            light = self.backends[msg['uniqueName']]
            light.status_setter(msg)
            self.rooms[light.room].fetch()  # update room

            # send new status as response
            response = self.status(msg['uniqueName'])
            response_id = Light.STATUS_RESPONSE

        return response_id, response

    def __handle_all_lights_request(self, _):
        return Light.ALL_LIGHTS_RESPONSE, {'state': self.light_state_all}

    def __handle_all_lights_response(self, msg):
        try:
            state = msg['state']

            for light in self.backends.values():
                if state == 1:
                    light.status_setter({'state': True})
                else:
                    light.status_setter({'state': False})
                # update room and send status response for each light
                self.notifier(light.unique_name)
        except KeyError:
            logging.warning('Invalid \'ALL_LIGHTS_RESPONSE\' received.')

        # no response will be send since fetch() sends one
        return Light.NULL, dict()

    def __handle_room_request(self, msg):
        room = msg['room']
        state = msg['state']
        self.rooms[room].switch(state)

        for light in self.rooms[room].backends:
            self.on_push(Light.serviceid, Light.STATUS_RESPONSE,
                         self.status(light.unique_name))

        return Light.ROOM_RESPONSE, self.rooms[room].status()

    def __handle_rooms_list_request(self, _):
        response = dict()
        for room_name, room in self.rooms.items():
            response[room_name] = room.state

        return Light.ROOMS_LIST_RESPONSE, response
