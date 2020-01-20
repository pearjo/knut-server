from enum import IntEnum
from events import Events
# from knutserver import KnutTcpSocket
import logging


class MessageId(IntEnum):
    """Message IDs used by the light API."""
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


class Room(Events):
    def __init__(self, room):
        self.serviceid = int('0x02', 16)
        self.backends = list()
        self.room = room
        self.state = float()
        # use the push event to push new status to parent object
        self.__events__ = ('push')
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
            self.push(self.serviceid, MessageId.ROOM_RESPONSE, self.status())


class Light(Events):
    # TODO: add docstring
    def __init__(self, socket):
        self.socket = socket
        self.backends = dict()
        self.rooms = dict()
        self.serviceid = int('0x02', 16)
        self.light_state_all = float()
        self.__events__ = ('push')

    def add_backend(self, backend):
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
            self.rooms[backend.room].push += self.push

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
            self.push(self.serviceid, MessageId.ALL_LIGHTS_RESPONSE,
                      {'state': self.light_state_all})

    def request_handler(self, msg_id, payload):
        """Handles all light requests.

        All light requests are handled by this method. The client can send the
        *requests* defined in the ``MessageId``.

        """
        response = dict()
        response_id = MessageId.NULL
        logging.debug('Received light request.')

        if msg_id == MessageId.STATUS_REQUEST:
            response_id, response = self.handle_status_request(payload)
        elif msg_id == MessageId.LIGHTS_REQUEST:
            response_id, response = self.handle_lights_request(payload)
        elif msg_id == MessageId.STATUS_RESPONSE:
            response_id, response = self.handle_status_response(payload)
        elif msg_id == MessageId.ALL_LIGHTS_REQUEST:
            response_id, response = self.handle_all_lights_request(payload)
        elif msg_id == MessageId.ALL_LIGHTS_RESPONSE:
            response_id, response = self.handle_all_lights_response(payload)
        elif msg_id == MessageId.ROOMS_LIST_REQUEST:
            response_id, response = self.handle_rooms_list_request(payload)
        elif msg_id == MessageId.ROOM_REQUEST:
            response_id, response = self.handle_room_request(payload)

        self.fetch()  # update light_state_all
        return response_id, response

    def notifier(self, unique_name):
        # update room in which the light is located
        light = self.backends[unique_name]
        self.rooms[light.room].fetch()

        # push the message to registered objects
        self.push(self.serviceid, MessageId.STATUS_RESPONSE,
                  self.status(unique_name))

    def status(self, unique_name):
        """Status information.

        Returns the status information of the light with the unique name
        *unique_name*.

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

    def handle_status_request(self, payload):
        response = dict()
        response_id = MessageId.NULL

        if payload['uniqueName'] not in self.backends.keys():
            logging.warning('No light service with unique name \'%s\' known.'
                            % payload['uniqueName'])
        else:
            response = self.status(payload['uniqueName'])
            response_id = MessageId.STATUS_RESPONSE

        return response_id, response

    def handle_lights_request(self, payload):
        response = dict()
        response_id = MessageId.LIGHTS_RESPONSE

        for light in self.backends.keys():
            response[light] = self.status(light)

        return response_id, response

    def handle_status_response(self, payload):
        response = dict()
        response_id = MessageId.NULL

        if payload['uniqueName'] not in self.backends.keys():
            logging.warning('No light service with unique name \'%s\' known.'
                            % payload['uniqueName'])
        else:
            light = self.backends[payload['uniqueName']]
            light.status_setter(payload)
            self.rooms[light.room].fetch()  # update room

            # send new status as response
            response = self.status(payload['uniqueName'])
            response_id = MessageId.STATUS_RESPONSE

        return response_id, response

    def handle_all_lights_request(self, payload):
        """Handles a ``ALL_LIGHTS_REQUEST``.

        A ``ALL_LIGHTS_RESPONSE`` is send upon the request. The *payload*
        can be empty, since it will be ignored when only the state is
        requested. The response contains the key *state* with the all light
        state being either 0 for all off, 1 for all on and 0.5 for neither
        all on nor off.
        """
        return MessageId.ALL_LIGHTS_RESPONSE, {'state': self.light_state_all}

    def handle_all_lights_response(self, payload):
        """Handles a ``ALL_LIGHTS_RESPONSE``.

        Handles a ``ALL_LIGHTS_RESPONSE``. The state of each backend
        light will be set to the state parsed with the *payload*. This triggers
        also the rooms to be updated and to send a ``ROOM_RESPONSE`` if their
        state changed. The *payload* needs to have the key ``state`` with an
        float being either 0 or 1.
        """
        try:
            state = payload['state']

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
        return MessageId.NULL, dict()

    def handle_room_request(self, payload):
        """Handles a room request."""
        room = payload['room']
        state = payload['state']
        self.rooms[room].switch(state)

        for light in self.rooms[room].backends:
            self.push(self.serviceid, MessageId.STATUS_RESPONSE,
                      self.status(light.unique_name))

        return MessageId.ROOM_RESPONSE, self.rooms[room].status()

    def handle_rooms_list_request(self, payload):
        response = dict()
        for room_name, room in self.rooms.items():
            response[room_name] = room.state

        return MessageId.ROOMS_LIST_RESPONSE, response
