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
import knut.services.light
import logging


class Room(Events):
    """Bundles :class:`~knut.services.Light` objects in a room."""

    apiId = 0x02
    """The room service id. The id is the same as the light service id since
    this service is part of the light service."""

    def __init__(self, room: str) -> None:
        """Using the room object, all lights within one *room* can be controlled
        at once."""

        self.backends = list()
        """A dictionary with all back-ends in the room where the keys are the
        :attr:`knut.services.Light.id` and the values are the
        corresponding light objects :class:`knut.services.Light`.
        """

        self.room = room
        """The room name."""

        self.state = int()
        """The state of the room where -1 is all lights are off, 1 all lights on
        and 0 when neither all lights are on nor off."""

        # use the on_push event to push new status to parent object
        self.__events__ = ('on_push')

        logging.debug('Added room to bundle lights: {}'.format(self.room))

    def add_backend(self, backend: knut.services.light.Light) -> None:
        """Adds a light *backend* to the room."""
        if backend.room != self.room:
            logging.warning("Back-end is not in room '{}': {}"
                            .format(self.room, backend.id))
            return

        if backend not in self.backends:
            self.backends.append(backend)
            self.fetch()

    def switch(self, state: float) -> None:
        """Switches all lights in :attr:`backends` on or off."""
        for light in self.backends:
            if state > 0:
                light.status_setter({'state': True})
            else:
                light.status_setter({'state': False})

        self.state = state

    def status(self) -> dict:
        """Returns the room's state.

        The returned dictionary has the following keys:

        - The identifier ``'id'`` of the room.
        - The ``'state'`` where *-1* is off, *1* on and *0* neither all on or
          off.

        """
        return {'id': self.room,
                'state': self.state}

    def fetch(self) -> None:
        """Fetch light states of all back-ends.

        Fetch the light states of all back-ends in the room and sets the
        :attr:`state` accordingly to -1 if all off, to 1 if all on or to 0
        if neither all are on nor off. If the :attr:`state` changed, a
        :const:`ROOM_RESPONSE` will be pushed.
        """
        state_set = set((light.state) for light in self.backends)

        # If all light states are equal, the set has one entry being the state.
        # Using pop() returns that entry and the light state can be set.
        if len(state_set) == 1:
            state = 1 if state_set.pop() else -1
        else:
            state = 0

        if self.state != state:
            self.state = state
            # push new status to registered event listeners
            self.on_push(Room.apiId, Light.ROOM_RESPONSE, self.status())


class Light(KnutAPI):
    """This class allows interaction with the light service. The following message
    types are handled by the ``request_handler()``:

    - :const:`LIGHT_STATUS_REQUEST`
    - :const:`LIGHT_STATUS_RESPONSE`
    - :const:`LIGHTS_REQUEST`
    - :const:`ALL_LIGHTS_REQUEST`
    - :const:`ALL_LIGHTS_RESPONSE`
    - :const:`ROOMS_LIST_REQUEST`
    - :const:`ROOM_REQUEST`

    .. note::

       Each light service back-end add to the dictionary must subclass the
       :class:`~knut.services.light.Light` class.

    """
    LIGHT_STATUS_REQUEST = 0x0001
    LIGHT_STATUS_RESPONSE = 0x0101
    LIGHTS_REQUEST = 0x0002
    LIGHTS_RESPONSE = 0x0102
    ALL_LIGHTS_REQUEST = 0x0003
    ALL_LIGHTS_RESPONSE = 0x0103
    ROOMS_LIST_REQUEST = 0x0004
    ROOMS_LIST_RESPONSE = 0x0104
    ROOM_REQUEST = 0x0005
    ROOM_RESPONSE = 0x0105

    apiId = 0x02

    def __init__(self):
        super(Light, self).__init__()
        self.supported = {
            Light.LIGHT_STATUS_REQUEST: self.__handle_status_request,
            Light.LIGHT_STATUS_RESPONSE: self.__handle_status_response,
            Light.LIGHTS_REQUEST: self.__handle_lights_request,
            Light.ALL_LIGHTS_REQUEST: self.__handle_all_lights_request,
            Light.ALL_LIGHTS_RESPONSE: self.__handle_all_lights_response,
            Light.ROOMS_LIST_REQUEST: self.__handle_rooms_list_request,
            Light.ROOM_REQUEST: self.__handle_room_request
        }

        self.backends = dict()
        """A dictionary with all back-ends where the keys are the
        identifiers of the back-ends and the values are the
        corresponding light objects :class:`~knut.services.Light`.
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
                    hasattr(backend, 'id')]):
            raise AttributeError("Back-end is missing at least on of the "
                                 "following attributes: ['temperature', "
                                 "'color', 'dimlevel', 'state', "
                                 "'location', 'id'")

        if backend.id in self.backends.keys():
            logging.warning('Back-end identifier is not unique: {}'
                            .format(backend.id))
            return

        self.backends[backend.id] = backend

        # add room to rooms dictionary
        if backend.room not in self.rooms.keys():
            self.rooms[backend.room] = Room(backend.room)
            self.rooms[backend.room].on_push += self.on_push

        self.rooms[backend.room].add_backend(backend)

        # add the notifier method to the back-end's on_change event
        if callable(self.backends[backend.id].on_change):
            self.backends[backend.id].on_change += self.notifier

    def fetch(self):
        state_set = set((light.state) for light in self.backends.values())

        # If all light states are equal, the set has one entry being the state.
        # Using pop() returns that entry and the light state can be set.
        if len(state_set) == 1:
            light_state_all = 1 if state_set.pop() else -1
        else:
            light_state_all = 0

        if self.light_state_all != light_state_all:
            self.light_state_all = light_state_all
            # push response to all clients
            self.on_push(Light.apiId, Light.ALL_LIGHTS_RESPONSE,
                         {'state': self.light_state_all})

    def request_handler(self, msg_id, msg):
        # override of the request_handler
        response_id, response = super(Light, self).request_handler(msg_id, msg)

        self.fetch()  # update light_state_all

        # check if the response is valid
        response_id = response_id if len(response) > 0 else Light.NULL

        return response_id, response

    def notifier(self, id):
        """Pushes the :meth:`status()` of the back-end with the *id* to all
        listeners of the ``on_push()`` event.
        """
        # update room in which the light is located
        light = self.backends[id]
        self.rooms[light.room].fetch()

        # push the message to registered objects
        logging.debug('Push status to listeners: {}'.format(id))
        self.on_push(Light.apiId, Light.LIGHT_STATUS_RESPONSE,
                     self.backends[id].status())

    def __handle_status_request(self, msg):
        response = dict()
        response_id = Light.NULL

        light_id = msg['id']

        if light_id not in self.backends.keys():
            logging.warning('Unknown light service requested: {}'
                            .format(light_id))
        else:
            response = self.backends[light_id].status()
            response_id = Light.LIGHT_STATUS_RESPONSE

        return response_id, response

    def __handle_lights_request(self, _):
        backends = list()
        response_id = Light.LIGHTS_RESPONSE

        for light in self.backends.values():
            backends.append(light.status())

        logging.debug(backends)

        response = {'backends': backends}
        return response_id, response

    def __handle_status_response(self, msg):
        response = dict()
        response_id = Light.NULL

        light_id = msg['id']

        if light_id not in self.backends.keys():
            logging.warning('Unknown light service requested: {}'
                            .format(light_id))
        else:
            light = self.backends[light_id]
            light.status_setter(msg)
            self.rooms[light.room].fetch()  # update room

            # send new status as response
            response = light.status()
            response_id = Light.LIGHT_STATUS_RESPONSE

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
                self.notifier(light.id)
        except KeyError:
            logging.warning("Invalid 'ALL_LIGHTS_RESPONSE' received.")

        # no response will be send since fetch() sends one
        return Light.NULL, dict()

    def __handle_room_request(self, msg):
        room = msg['id']
        state = msg['state']
        self.rooms[room].switch(state)

        for light in self.rooms[room].backends:
            self.on_push(Light.apiId, Light.LIGHT_STATUS_RESPONSE,
                         light.status())

        return Light.ROOM_RESPONSE, self.rooms[room].status()

    def __handle_rooms_list_request(self, _):
        rooms = list()

        for room_name, room in self.rooms.items():
            rooms.append(room.status())

        response = {'rooms': rooms}
        return Light.ROOMS_LIST_RESPONSE, response
