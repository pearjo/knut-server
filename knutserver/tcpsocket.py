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
import socket
import json
import logging
import select
import threading
import queue

ENCODING = 'utf-8'


class KnutTcpSocket():
    """The Knut TCP socket.

    The Knut server is bound to *host* with the specified *port*.

    Clients can communicate with the server using JSON formatted messages.
    The message is constructed out of the following two parts:

    1. A unsigned 32 bit long integer defining the JSON message length
    2. The UTF-8 encoded JSON message

    The JSON message itself has the following keys:

    ``serviceId``

       The service id of the targeted Knut service. Each API handles requests
       for a specific service id. The following table lists the service ids and
       the corresponding APIs.

       +------------+-------------------------------+
       | Service Id | API                           |
       +============+===============================+
       |   ``0x01`` | :class:`knutapis.Temperature` |
       +------------+-------------------------------+
       |   ``0x02`` | :class:`knutapis.Light`       |
       +------------+-------------------------------+

    ``msgId``

       The id of the message. Each API defines which messages it can understand
       and what ids they have. Read the API documentation to get more
       information about the supported messages and their ids.

    ``msg``

       The message as JSON formatted string and encoded as UTF-8.

    For example, lets assume we have a temperature back-end called
    ``myTempSensor`` and we want to request the current status, we would need
    to send the following request from the client side::

       {"seviceId": 1, "msgId": 1, "msg": {"uniqueName": "myTempSensor"}}

    The message length of the example message is ``0x0000003e``.

    For each received message, a request handler is called to process the
    incoming request by the client and to send a proper response.
    """
    def __init__(self, host='localhost', port=8080):
        logging.info('Open server on socket %s:%i...' % (host, port))
        self.services = dict()
        self.addr = (host, port)
        self.serversocket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.serversocket.setblocking(0)
        self.serversocket.bind(self.addr)
        self.serversocket.listen(5)

        self._in_sockets = [self.serversocket]  # sockets to read from
        self._out_sockets = list()  # sockets to write to
        self._out_msg_queues = dict()  # outgoing message queues
        self._ready_in_sockets = list()  # sockets that are actually readyble
        self._ready_out_sockets = list()  # sockets that are actually writable

        listener_thread = threading.Thread(target=self.listener,
                                           name='listener_thread')
        listener_thread.daemon = True
        listener_thread.start()

    def add_service(self, service):
        """Add a service to the server socket.

        The parsed *service* is added to the dictionary of services and it's
        ``push`` event is connected to the sockets ``send`` method.
        """
        if not all([hasattr(service, 'serviceid'),
                    hasattr(service, 'push')]):
            raise AttributeError('The service \'%s\' is missing either a'
                                 + '\'serviceid\' or a \'push\' event.')

        service_id = service.serviceid
        self.services[service_id] = service
        # add the notifier method to the back-end's on_change event
        if callable(self.services[service_id].push):
            self.services[service_id].push += self.send

    def listener(self):
        """Listen for connections and manages in- and outgoing messages."""
        while self._in_sockets:
            logging.info('Server is listening for connections...')
            self._ready_in_sockets, self._ready_out_sockets, error_sockets = \
                select.select(self._in_sockets,
                              self._out_sockets,
                              self._in_sockets)

            # input handler
            for in_socket in self._ready_in_sockets:
                self._input_socket_handler(in_socket)

            # output handler
            for out_socket in self._ready_out_sockets:
                self._output_socket_handler(out_socket)

            # error handler
            for in_error in error_sockets:
                logging.debug('Cleanup socket %s with error...'
                              % str(in_error.getpeername()))
                self._in_sockets.remove(in_error)
                if in_error in self._out_sockets:
                    self._out_sockets.remove(in_error)

                in_error.close()
                del self._out_msg_queues[self.in_error]

    def _input_socket_handler(self, in_socket):
        """Handles an input socket *in_socket*."""
        if in_socket is self.serversocket:
            clientsocket, clientaddr = in_socket.accept()
            clientsocket.setblocking(0)

            # add socket to input sockets
            self._in_sockets.append(clientsocket)

            # queue for data to send
            self._out_msg_queues[clientsocket] = queue.Queue()
        else:
            byte_response = self.client_reader(in_socket)

            if byte_response:
                # add response to out queue
                self._out_msg_queues[in_socket].put(byte_response)

                # add socket to output sockets
                if in_socket not in self._out_sockets:
                    self._out_sockets.append(in_socket)
            else:
                self._in_sockets.remove(in_socket)
                if in_socket in self._out_sockets:
                    self._out_sockets.remove(in_socket)

                # close socket and delete from outgoing message queue
                in_socket.close()
                del self._out_msg_queues[in_socket]

    def _output_socket_handler(self, out_socket):
        """Handles an *out_socket* and sends message via the socket."""
        try:
            # get next message from the queue
            next_msg = self._out_msg_queues[out_socket].get_nowait()
            logging.debug('Send %s to %s...' % (next_msg,
                                                str(out_socket.getpeername())))
            out_socket.sendall(next_msg)
        except (queue.Empty,
                KeyError,
                OSError):
            logging.debug('Remove output socket...')
            self._out_sockets.remove(out_socket)
            self._ready_out_sockets.remove(out_socket)

    def client_reader(self, clientsocket):
        """Returns a Knut response message upon a clients message.

        If the *clientsocket* has data to read, those will be read and parsed
        to the :meth:`request_handler()`. The response upon the clients message
        is then returned as Knut message.
        """
        byte_data = bytearray()
        byte_response = bytearray()
        data = dict()
        response = dict()
        response_id = 0x0000

        try:
            # read the first to message size bytes to check if connection
            # is open
            msg_size_bytes = clientsocket.recv(4)

            # if no header is read, return none
            if not msg_size_bytes:
                return

            msg_size = int.from_bytes(msg_size_bytes, byteorder='big')
            logging.debug('Received %i bytes.' % msg_size)

            if msg_size > 0:
                byte_data = clientsocket.recv(msg_size)
                logging.debug('Received raw bytes %s.'
                              % str(byte_data))
                data = json.loads(byte_data, encoding=ENCODING)
                logging.debug(str('Received ' + str(data)))

                msg_id = data['msgId']
                service_id = data['serviceId']

                logging.debug('Received message id %i for service %i.'
                              % (msg_id, service_id))

                response_id, response = self.request_handler(service_id,
                                                             msg_id,
                                                             data['msg'])
        except (json.decoder.JSONDecodeError,
                ValueError,
                ConnectionResetError,
                TypeError):
            logging.warning('Failed to decode JSON message.')

        if response_id > 0:
            byte_response = self.msg_builder(service_id, response_id, response)

        return byte_response

    def request_handler(self, service_id, msg_id, payload):
        """Handles the data of a valid request.

        This method calls the :meth:`request_handler` method of the
        corresponding service. The services returned response is then return by
        this method.
        """
        if service_id not in self.services.keys():
            logging.warning('No known service \'%s\'.' % hex(service_id))
            return msg_id, payload

        return self.services[service_id].request_handler(msg_id, payload)

    def send(self, service_id, msg_id, msg):
        """Sends the message *msg* to all open sockets."""
        byte_msg = self.msg_builder(service_id, msg_id, msg)

        for out_socket in self._ready_out_sockets:
            self._out_msg_queues[out_socket].put(byte_msg)
            self._output_socket_handler(out_socket)

    def msg_builder(self, service_id, msg_id, msg):
        """Returns *msg* as Knut message with header as byte array."""
        data = dict()

        data['serviceId'] = service_id
        data['msgId'] = msg_id
        data['msg'] = msg

        data_str = json.dumps(data)
        msg_size = len(data_str).to_bytes(4, byteorder='big')

        logging.debug('Build %i byte long message %s.'
                      % (len(data_str), str(data_str)))

        return msg_size + bytearray(data_str, ENCODING)

    def exit(self):
        """Close the server socket."""
        logging.info('Close server socket.')
        self.serversocket.close()
        self._in_sockets.remove(self.serversocket)
