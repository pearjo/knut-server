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

from typing import Tuple
import asyncio
import json
import logging

import websockets

from .knutserver import KnutServer
import knut.apis


class KnutWebSocket(KnutServer):
    """WebSocket for Knut."""

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
            except websockets.exceptions.ConnectionClosedError:
                logging.debug('Connection unexpected closed.')
                return

            if msgid > 0:
                data = {'apiId': apiid, 'msgId': msgid, 'msg': msg}
                await websocket.send(json.dumps(data))

    def request_service(self,
                        apiid: int,
                        msgid: int,
                        msg: dict) -> Tuple[int, dict]:
        """Return a service's response.

        Call the ``request_handler()`` of the API with the *apiid*. The API's
        request handler handles then the *msg* of type *msgid*. Return a tuple
        that holds the responses *msgid* and *msg*. If no API for *apiid* is
        found in the servers API dictionary, the *msg* and *msgid* is returned.
        """

        if apiid not in self.apis.keys():
            logging.warning('Unknown API request: {}'.format(apiid))
            return msgid, msg

        return self.apis[apiid].request_handler(msgid, msg)

    def knut_serve_forever(self):
        server = websockets.serve(self.request_handler,
                                  self.address,
                                  self.port)
        asyncio.get_event_loop().run_until_complete(server)
        asyncio.get_event_loop().run_forever()
