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
import logging


class Room(Events):
    serviceid = 0x02

    def __init__(self, room):
        self.backends = list()
        self.room = room
        self.state = float()
        """The state of the room where 0 is all lights are off, 1 all lights on
        and 0.5 when neither all lights are on nor off."""
        # use the on_push event to push new status to parent object
        self.__events__ = ('on_push')
        logging.debug('Added room \'%s\' to bundle lights' % self.room)

    def add_backend(self, backend):
        """Add a light backend."""
        if backend.room != self.room:
            logging.warning('Data backend \'%s\' is not in room \'%s\''
                            % (backend.unique_name, self.room))
            return

        if backend not in self.backends:
            self.backends.append(backend)
            self.fetch()

    def switch(self, state):
        """Switch all lights of the room on or off by setting the *state*."""
        for light in self.backends:
            if state == 1:
                light.status_setter({'state': True})
            else:
                light.status_setter({'state': False})

        self.state = state

    def status(self):
        """Returns the room state.

        Returns a dictionary with the following keys:

        * room
        * state

        The state is 0 if all lights are off, 1 if all lights are on and
        0.5 if neither all lights are on nor off.
        """
        return {'room': self.room,
                'state': self.state}

    def fetch(self):
        """Fetch light states of all backends.

        Fetch the light states of all backends in the room and sets the
        *state* accordingly to 0 if all off, to 1 if all on or to 0.5
        if neither all are on nor off. If the *state* changed, a
        ``ROOM_RESPONSE`` will be pushed.
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


class Light(Events):
    """Knut light API.

    The light object has a dictionary :attr:`backends` of all available
    back-ends. A back-end can be added using :meth:`add_backend()`.  To interact
    with the back-ends, this API has as request handler method
    :meth:`request_handler()`. Using this method, requests can be send to the
    back-ends to get e.g. the status of all lights.

    The following message types *msg_id* are supported with the required
    message *msg* and their *response* with its *response_id*:

    .. py:data:: STATUS_REQUEST
       :value: 0x0001

       Requests the status of a back-end. The message *msg* must have the
       key *uniqueName*. For example::

          {"uniqueName": "myLightBackend"}

    .. py:data:: STATUS_RESPONSE
       :value: 0x0101

       The status *response* is the dictionary returned by :meth:`status()`.
       For example::

          {
              "uniqueName": "myLightBackend",
              "location": "Somewhere in a room...",
              "room": "Some room",
              "state": false,
              "hasTemperature": true,
              "hasDimlevel": true,
              "hasColor": false,
              "temperature": 100,
              "colorCold": "#f5faf6",
              "colorWarm": "#efd275",
              "dimlevel": 50,
              "color": ""
          }

    .. py:data:: LIGHTS_REQUEST
       :value: 0x0002

       Requests the status of all back-ends. The message *msg* can be emtpy.

    .. py:data:: LIGHTS_RESPONSE
       :value: 0x0102

       The *response* of the lights response is similar to the
       :const:`STATUS_RESPONSE`, only with all known back-ends that are in
       :attr:`backends`. For example::

          {
              "myLightBackend1": {
                  "location": "Somewhere in a room...",
                  "room": "Some room",
                  "state": false,
                  "hasTemperature": true,
                  "hasDimlevel": true,
                  "hasColor": false,
                  "temperature": 100,
                  "colorCold": "#f5faf6",
                  "colorWarm": "#efd275",
                  "dimlevel": 50,
                  "color": ""
              },
              "myLightBackend1": {
                  "location": "Somewhere else in a room...",
                  "room": "Some other room",
                  "state": true,
                  "hasTemperature": true,
                  "hasDimlevel": true,
                  "hasColor": false,
                  "temperature": 100,
                  "colorCold": "#f5faf6",
                  "colorWarm": "#efd275",
                  "dimlevel": 50,
                  "color": ""
              }
          }

    .. py:data:: ALL_LIGHTS_REQUEST
       :value: 0x0003

       Requests the combined state of all lights. The message *msg* can be
       emtpy.

    .. py:data:: ALL_LIGHTS_RESPONSE
       :value: 0x0103

       The all lights *response* is a dictionary with one key *state*, where
       *0* is all lights off, *1* all lights on and *0.5* where neither all
       lights are on nor off. A client can send this response to switch all
       lights on or off. For example::

          {"state": 1.0}

    .. py:data:: ROOMS_LIST_REQUEST
       :value: 0x0004

       Requests a list of all rooms with their state. The message *msg* can
       be emtpy.

    .. py:data:: ROOMS_LIST_RESPONSE
       :value: 0x0104

       The rooms list *response* is a dictionary with the unique names of
       the rooms as keys and the :attr:`knut.apis.light.Room.state` as values::

          {
              "myRoom1": 0.5,
              "myRoom2": 0.0
          }

    .. py:data:: ROOM_REQUEST
       :value: 0x0005

       Requests to set the :attr:`knut.apis.light.Room.state` for a room.
       The keys *room* and *state* are required, where only the states
       *0* and *1* are applied. A request should look like::

          {
              "room": "myRoom1",
              "state": 0
          }

    .. py:data:: ROOM_RESPONSE
       :value: 0x0105

       The *response* dictionary is the dictionary returned by
       :meth:`knut.apis.light.Room.status`

    """
    NULL = 0x0000
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
    """The light service id."""

    def __init__(self):
        self.backends = dict()
        """A dictionary with all back-ends where the keys are the ``unique_name`` and
        the values are the corresponding light objects
        :class:`knut.services.Light`
        """
        self.rooms = dict()
        self.light_state_all = float()
        self.__events__ = ('on_push')

    def add_backend(self, backend):
        """Adds the *backend* to :attr:`backends`."""
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
        """Returns the tuple (*response_id*, *response*) upon a request.

        Handles the requested *msg* of type *msg_id* and returns a *response* of
        type *response_id*. See :py:class:`Light` for a list of all supported
        messages.
        """
        response = dict()
        response_id = Light.NULL
        logging.debug('Received light request.')

        if msg_id == Light.STATUS_REQUEST:
            response_id, response = self._handle_status_request(msg)
        elif msg_id == Light.LIGHTS_REQUEST:
            response_id, response = self._handle_lights_request(msg)
        elif msg_id == Light.STATUS_RESPONSE:
            response_id, response = self._handle_status_response(msg)
        elif msg_id == Light.ALL_LIGHTS_REQUEST:
            response_id, response = self._handle_all_lights_request(msg)
        elif msg_id == Light.ALL_LIGHTS_RESPONSE:
            response_id, response = self._handle_all_lights_response(msg)
        elif msg_id == Light.ROOMS_LIST_REQUEST:
            response_id, response = self._handle_rooms_list_request(msg)
        elif msg_id == Light.ROOM_REQUEST:
            response_id, response = self._handle_room_request(msg)

        self.fetch()  # update light_state_all

        # check if the response is valid
        response_id = response_id if len(response) > 0 else Light.NULL

        return response_id, response

    def notifier(self, unique_name):
        """Pushes the :meth:`status()` of *unique_name* to all listeners of the
        :meth:`on_push` event."""
        # update room in which the light is located
        light = self.backends[unique_name]
        self.rooms[light.room].fetch()

        # push the message to registered objects
        logging.debug('Push status of \'%s\' to listeners.' % unique_name)
        self.on_push(Light.serviceid, Light.STATUS_RESPONSE,
                     self.status(unique_name))

    def status(self, unique_name):
        """Returns the status information of the light *unique_name*.

        The status dictionary has the keys

        - *color*
        - *colorCold*
        - *colorWarm*
        - *dimlevel*
        - *hasColor*
        - *hasDimlevel*
        - *hasTemperature*
        - *location*
        - *room*
        - *state*
        - *temperature*
        - *uniqueName*

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

    def _handle_status_request(self, msg):
        response = dict()
        response_id = Light.NULL

        if msg['uniqueName'] not in self.backends.keys():
            logging.warning('No light service with unique name \'%s\' known.'
                            % msg['uniqueName'])
        else:
            response = self.status(msg['uniqueName'])
            response_id = Light.STATUS_RESPONSE

        return response_id, response

    def _handle_lights_request(self, msg):
        response = dict()
        response_id = Light.LIGHTS_RESPONSE

        for light in self.backends.keys():
            response[light] = self.status(light)

        return response_id, response

    def _handle_status_response(self, msg):
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

    def _handle_all_lights_request(self, msg):
        return Light.ALL_LIGHTS_RESPONSE, {'state': self.light_state_all}

    def _handle_all_lights_response(self, msg):
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

    def _handle_room_request(self, msg):
        room = msg['room']
        state = msg['state']
        self.rooms[room].switch(state)

        for light in self.rooms[room].backends:
            self.on_push(Light.serviceid, Light.STATUS_RESPONSE,
                         self.status(light.unique_name))

        return Light.ROOM_RESPONSE, self.rooms[room].status()

    def _handle_rooms_list_request(self, msg):
        response = dict()
        for room_name, room in self.rooms.items():
            response[room_name] = room.state

        return Light.ROOMS_LIST_RESPONSE, response
