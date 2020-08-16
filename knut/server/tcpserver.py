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
from knut.apis import KnutAPI
from typing import Tuple
import json
import logging
import queue
import socketserver
import threading

from .knutserver import KnutServer

class KnutTCPRequestHandler(socketserver.BaseRequestHandler):
    """The request handler class for the :class:`knut.server.KnutTCPServer`.

    It is instantiated once per connection to the server and kept open until
    either the client closes the connection or the server is shutdown.
    """
    ENCODING = 'utf-8'
    """The encoding in which messages are received and send."""

    HEARTBEAT_FREQUENCY = 1
    """The frequency in which the heartbeat is send measured in Hz. See
    :meth:`heartbeat()` for more about the heartbeat."""

    def __init__(self, request, client_address, server):
        self.__msg_queue = queue.Queue()

        self.send_heartbeat = True
        """If true, :meth:`heartbeat()` is called with a frequency of
        :const:`HEARTBEAT_FREQUENCY`."""

        super(KnutTCPRequestHandler, self).__init__(request,
                                                    client_address,
                                                    server)

    def finish(self) -> None:
        self.send_heartbeat = False
        logging.debug('Close request handle: {}'.format(self.client_address))

    def handle(self) -> None:
        logging.debug('Handle client request: {}'.format(self.client_address))

        while True:
            data = b''
            knutmsg = dict()
            msg = dict()
            msgid = 0x0000
            apiid = 0x00

            try:
                # read until a null byte is received
                while True:
                    read_byte = self.request.recv(1)
                    if read_byte == b'\x00':
                        break
                    elif not read_byte:
                        return  # no data available anymore
                    else:
                        data += read_byte

                logging.debug('Received bytes: {}'.format(len(data)))

                if len(data) > 0:
                    logging.debug('Received raw message: {}'.format(data))
                    knutmsg = json.loads(
                        data,
                        encoding=KnutTCPRequestHandler.ENCODING
                    )

                    try:
                        apiid = knutmsg['apiId']
                        msgid = knutmsg['msgId']
                        msg = knutmsg['msg']

                        logging.debug('Message of type {} for API {} '
                                      'received...'.format(msgid, apiid))

                        msgid, msg = self.request_service(
                            apiid, msgid, msg)
                    except KeyError:
                        logging.warning('Received message is missing at least '
                                        'one of the following keys: '
                                        '[msgId, apiId, msg]')

            except json.decoder.JSONDecodeError:
                logging.warning('Failed to decode JSON message...')

            if msgid > 0:
                self.send_queued_knutmsg(apiid, msgid, msg)

    def heartbeat(self) -> None:
        """Sends frequently a heartbeat.

        The heartbeat is a frequently send message, where only a null byte
        ``b'\\x00'`` is send.
        """
        if self.send_heartbeat:
            self.send_queued(b'\x00')
            timer = threading.Timer(
                1 / KnutTCPRequestHandler.HEARTBEAT_FREQUENCY,
                self.heartbeat
            )
            timer.daemon = True
            timer.start()

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
        if apiid not in self.server.apis.keys():
            logging.warning('Unknown API request: {}'.format(apiid))
            return msgid, msg

        return self.server.apis[apiid].request_handler(msgid, msg)

    def send_queued_knutmsg(self, apiid: int, msgid: int, msg: dict) -> None:
        """Sends a queued Knut message.

        Sends the *msg* of type *msgid* from the *apiid* as a queued Knut
        message.
        """
        self.send_queued(knutmsg_builder(apiid,
                                         msgid,
                                         msg,
                                         KnutTCPRequestHandler.ENCODING))

    def send_queued(self, msg: bytearray) -> None:
        """Sends a queued message.

        Puts the *msg* to the message queue and sends all messages form the
        queue until its empty.
        """
        self.__msg_queue.put(msg)

        while not self.__msg_queue.empty():
            try:
                # get next message from the queue
                next_msg = self.__msg_queue.get_nowait()

                # don't log heartbeats
                if next_msg != b'\x00':
                    logging.debug('Send message from queue to client {}: {}'
                                  .format(self.client_address, next_msg))

                self.request.sendall(next_msg)
            except (BrokenPipeError, OSError):
                return

    def setup(self) -> None:
        logging.debug('Start heartbeat: {}'.format(self.client_address))
        threading.Thread(target=self.heartbeat, daemon=True).start()

        for apiid, service in self.server.apis.items():
            service.on_push += self.send_queued_knutmsg


class KnutTCPServer(socketserver.ThreadingMixIn,
                    socketserver.TCPServer,
                    KnutServer):
    """The Knut TCP server class.

    The server handles all communication from clients and redirects requests to
    the APIs of the corresponding services. For each client, the request handler
    :class:`~knut.server.tcpserver.KnutTCPRequestHandler` is instantiated in a
    new thread. The connection is kept open once a request is received to allow
    sending push notifications via TCP back to the connected client. See
    :meth:`add_api()` for more about how to add an API to the server.

    The request handler reads UTF-8 encoded JSON messages terminated by a null
    byte ``b'\\x00'``. See :ref:`knutmsg` for more details.

    After adding APIs, the server can be run by ``serve_forever()``. The
    request are then handled by the request handler until the server is
    shutdown. See the documentation for the `socketserver
    <https://docs.python.org/3/library/socketserver.html#module-socketserver>`_
    module for details.

    As an example, a :class:`KnutTCPServer` is instantiated and
    :class:`knut.apis.Temperature` is add to the server's APIs. The server is
    run for one minute and using ``netcat`` as client, a request is send to the
    server::

       from knut.apis import Temperature
       from knut.server import KnutTCPServer
       from knut.services.dummytemperature import DummyTemperature
       import threading
       import time

       # add the DummyTemperature service back-end to the API
       temperature = Temperature()
       temperature.add_backend(DummyTemperature('Somewhere', 'dummy'))

       server = KnutTCPServer(("localhost", 8080))
       server.add_api(temperature)

       with server:
           server_thread = threading.Thread(target=server.serve_forever)
           server_thread.daemon = True
           server_thread.start()
           time.sleep(60)  # run for one minute

       server.shutdown()

    From the client, a :const:`TEMPERATURE_LIST_REQUEST` is send to the API:

    .. code-block:: bash

       $ echo -ne '{"apiId": 1, "msgId": 2, "msg": {}}\\0' | netcat localhost 8080
       {"apiId": 1, "msgId": 258, "msg": {"dummy": {"location": "Somewhere",
           "unit": "\\u00b0C", "condition": "\\uf002", "temperature": 10.2}}}

    The server finally responses with the API's
    :const:`TEMPERATURE_LIST_RESPONSE`.

    """

    def __init__(self, address: str = "127.0.0.1", port: int = 8080) -> None:
        """The server is bound to the *address* on the specified *port*."""
        self.allow_reuse_address = True

        self.apis = dict()

        super(KnutTCPServer, self).__init__((address, port),
                                            KnutTCPRequestHandler)

    def knut_serve_forever(self):
        with self:
            server_thread = threading.Thread(target=self.serve_forever)
            server_thread.daemon = True
            server_thread.start()
            server_thread.join()


def knutmsg_builder(apiid: int,
                    msgid: int,
                    msg: dict,
                    encoding: str) -> bytearray:
    """Returns a Knut message.

    Builds a Knut message for the API *apiid* and the message *msg* of
    type *msgid*. The Knut message is terminated by an null byte ``b'\\x00'``.

    For example::

       >>> import knut.server.tcpserver
       >>> knut.server.tcpserver.knutmsg_builder(2, 2, {}, 'utf-8')
       bytearray(b'{"apiid": 2, "msgId": 2, "msg": {}}\\x00')

    """
    data = {'apiId': apiid, 'msgId': msgid, 'msg': msg}
    data_str = json.dumps(data)

    logging.debug('Build {} byte long message: {}'
                  .format(len(data_str), data_str))

    return bytearray(data_str, encoding) + b'\x00'
