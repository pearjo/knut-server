from knut.core import KnutConfig
from typing import Tuple
import asyncio
import json
import knut.apis
import logging
import websockets


class KnutWebSocket():
    def __init__(self) -> None:
        self.apis = dict()
        """A dictionary with all APIs as value and their corresponding service
        ids as keys. See :meth:`add_api()` for more about adding an API.
        """

    def add_api(self, api: knut.apis.KnutAPI) -> None:
        """Adds a *api* to the server.

        The *api* is added to the dictionary of APIs and it's ``on_push()``
        event is connected to the servers request handler to push messages.

        The request handler will also call the APIs ``request_handler()`` method
        if the API's service matches the requested service.
        """
        if not all([hasattr(api, 'apiid'),
                    hasattr(api, 'on_push')]):
            raise AttributeError('API is missing either a \'apiid\' or '
                                 'an \'on_push\' event: {}'.format(api))

        apiid = api.apiid
        self.apis[apiid] = api

    async def request_handler(self, websocket, path):
        while True:
            msg = dict()
            msgid = 0
            apiid = 0

            try:
                data = await websocket.recv()

                if len(data) > 0:
                    knutmsg = json.loads(data)

                    try:
                        apiid = knutmsg['apiId']
                        msgid = knutmsg['msgId']
                        msg = knutmsg['msg']

                        logging.debug('Message of type {} for API {} '
                                      'received...'.format(msgid, apiid))

                        msgid, msg = self.request_service(apiid, msgid, msg)
                    except KeyError:
                        logging.warning('Received message is missing at least '
                                        'one of the following keys: '
                                        '[msgId, apiId, msg]')

            except json.decoder.JSONDecodeError:
                logging.warning('Failed to decode JSON message...')

            if msgid > 0:
                data = {'apiId': apiid, 'msgId': msgid, 'msg': msg}
                await websocket.send(json.dumps(data))

    def request_service(self,
                        apiid: int,
                        msgid: int,
                        msg: dict) -> Tuple[int, dict]:
        """Returns a service's response.

        This method calls the ``request_handler()`` of the API with the
        *apiid*. The API's request handler handles then the *msg* of
        type *msgid*. The returned tuple holds the responses *msgid* and *msg*.
        If no API for *apiid* is found in the servers API
        dictionary, the *msg* and *msgid* is returned.
        """
        if apiid not in self.apis.keys():
            logging.warning('Unknown API request: {}'.format(apiid))
            return msgid, msg

        return self.apis[apiid].request_handler(msgid, msg)


socket = KnutWebSocket()

# load config
config = KnutConfig('../../etc/example.yml').config

# load task module
task = config['task']
task.load_tasks()
socket.add_api(task)

# load temperature module
try:
    temp = knut.apis.Temperature()
    for backend in config['temperature']:
        temp.add_backend(backend)

    socket.add_api(temp)
except KeyError:
    pass

# load light module
try:
    light = knut.apis.Light()
    for backend in config['lights']:
        light.add_backend(backend)

    socket.add_api(light)
except KeyError:
    pass

# load local module
if config['local']:
    local = knut.apis.Local()
    local.set_local(config['local'])
    socket.add_api(local)

if __name__ == '__main__':
    start_server = websockets.serve(socket.request_handler, "localhost", 8765)

    asyncio.get_event_loop().run_until_complete(start_server)
    asyncio.get_event_loop().run_forever()
