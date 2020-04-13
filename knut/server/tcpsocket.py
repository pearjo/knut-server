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
HEARTBEAT_FREQUENCY = 0.25  # send a heartbeat every 4 seconds

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

       +------------+--------------------------------+
       | Service Id | API                            |
       +============+================================+
       |   ``0x01`` | :class:`knut.apis.Temperature` |
       +------------+--------------------------------+
       |   ``0x02`` | :class:`knut.apis.Light`       |
       +------------+--------------------------------+
       |   ``0x03`` | :class:`knut.apis.Task`        |
       +------------+--------------------------------+

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

    .. note::
       A :meth:`heartbeat()` is send to all connected clients every 4 seconds,
       so that the client can check if it is still connected to Knut. The
       heartbeat is an empty message with only a message length of
       ``0x00000000``.

    """
    def __init__(self, host='localhost', port=8080):
        logging.info(str('Open server on socket %s:%i...' % (host, port)))
        self.services = dict()
        self.addr = (host, port)
        self.serversocket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.serversocket.setblocking(0)
        self.serversocket.bind(self.addr)
        self.serversocket.listen(5)

        self._in_sockets = [self.serversocket]  # sockets to read from
        self._out_sockets = list()  # sockets to write to
        self._out_msg_queues = dict()  # outgoing message queues
        self._known_clients = list()  # client sockets which might be open

        heartbeat_thread = threading.Thread(target=self.heartbeat,
                                            name='heartbeat_thread')
        heartbeat_thread.daemon = True
        logging.debug('Start the heartbeat...')
        heartbeat_thread.start()

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

    def heartbeat(self):
        """Sends frequently a heartbeat.

        The heartbeat is a frequently send message, where only a message length
        of ``0x00000000`` is send.

        The heartbeat is send to all available clients.
        """
        for client in self._known_clients:
            try:
                client.sendall(int(0).to_bytes(4, byteorder='big'))
            except OSError:
                logging.debug('Remove client from known clients list...')
                self._known_clients.remove(client)
                client.close()

        threading.Timer(1 / HEARTBEAT_FREQUENCY, self.heartbeat).start()

    def listener(self):
        """Listen for connections and manages in- and outgoing messages."""
        while self._in_sockets:
            logging.info('Server is waiting for events...')
            ready_in_sockets, ready_out_sockets, error_sockets = \
                select.select(self._in_sockets,
                              self._out_sockets,
                              self._in_sockets)

            # input handler
            for in_socket in ready_in_sockets:
                self._input_socket_handler(in_socket)

            # output handler
            for out_socket in ready_out_sockets:
                self._output_socket_handler(out_socket)

            # error handler
            for in_error in error_sockets:
                logging.debug('Cleanup socket with error...')
                self.remove_socket(in_error)

    def _input_socket_handler(self, in_socket):
        """Handles an input socket *in_socket*."""
        if in_socket is self.serversocket:
            clientsocket, clientaddr = in_socket.accept()
            logging.debug(str('Accepted connection from %s.'
                              % str(clientaddr)))
            clientsocket.setblocking(0)

            # add socket to input sockets and know clients
            self._in_sockets.append(clientsocket)
            self._known_clients.append(clientsocket)

            # queue for data to send
            self._out_msg_queues[clientsocket] = queue.Queue()
        else:
            byte_response = self.client_reader(in_socket)

            if byte_response:
                # add response to out queue
                self._out_msg_queues[in_socket].put(byte_response)

                # add socket to output sockets
                if in_socket not in self._out_sockets:
                    logging.debug(str('Add %s to outgoing sockets...'
                                      % str(in_socket.getpeername())))
                    self._out_sockets.append(in_socket)

    def _output_socket_handler(self, out_socket):
        """Handles an *out_socket* and sends message via the socket."""
        try:
            # get next message from the queue
            next_msg = self._out_msg_queues[out_socket].get_nowait()
            logging.debug(str('Send message to %s...'
                              % str(out_socket.getpeername())))
            out_socket.sendall(next_msg)
        except queue.Empty:
            logging.debug(str('Message queue for socket %s is empty.'
                              % str(out_socket.getpeername())))
            self._out_sockets.remove(out_socket)
        except (KeyError, OSError):
            self.remove_socket(out_socket)

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
            msg_size = int.from_bytes(msg_size_bytes, byteorder='big')
            logging.debug(str('Received %i bytes.' % msg_size))

            if msg_size > 0:
                byte_data = clientsocket.recv(msg_size)
                logging.debug(str('Received raw bytes %s.'
                                  % str(byte_data)))
                data = json.loads(byte_data, encoding=ENCODING)
                logging.debug(str('Received ' + str(data)))

                msg_id = data['msgId']
                service_id = data['serviceId']

                logging.debug(str('Received message id %i for service %i.'
                                  % (msg_id, service_id)))

                response_id, response = self.request_handler(service_id,
                                                             msg_id,
                                                             data['msg'])
            else:
                # if the message size is zero, remove the client from the input
                # sockets
                self.remove_socket(clientsocket)
        except json.decoder.JSONDecodeError:
            logging.warning('Failed to decode JSON message.')
        except ConnectionResetError:
            logging.debug('Connection reset by peer...')
            self.remove_socket(clientsocket)
        except MemoryError:
            logging.warning('Received message with invalid header.')
        except OSError as err:
            logging.error('OSError %s' % str(err))

        if response_id > 0:
            byte_response = msg_builder(service_id, response_id, response)

        return byte_response

    def remove_socket(self, clientsocket):
        """Remove the *clientsocket* from the list of in- and outgoing sockets."""
        try:
            peername = str(clientsocket.getpeername())
        except OSError:
            peername = 'unknown'
        finally:
            logging.debug(str('Remove %s from in- and outgoing sockets...'
                              % peername))

        if clientsocket in self._in_sockets:
            self._in_sockets.remove(clientsocket)
        if clientsocket in self._out_sockets:
            self._out_sockets.remove(clientsocket)
        if clientsocket in self._out_msg_queues.keys():
            del self._out_msg_queues[clientsocket]

        clientsocket.close()

    def request_handler(self, service_id, msg_id, payload):
        """Handles the data of a valid request.

        This method calls the :meth:`request_handler` method of the
        corresponding service. The services returned response is then return by
        this method.
        """
        if service_id not in self.services.keys():
            logging.warning(str('No known service \'%s\'.' % hex(service_id)))
            return msg_id, payload

        return self.services[service_id].request_handler(msg_id, payload)

    def send(self, service_id, msg_id, msg):
        """Sends the message *msg* to all open sockets."""
        byte_msg = msg_builder(service_id, msg_id, msg)

        for client in self._known_clients:
            try:
                logging.debug(str('Send message to %s...'
                                  % str(client.getpeername())))
                client.sendall(byte_msg)
            except OSError:
                logging.debug('Remove client from known clients list...')
                self._known_clients.remove(client)
                client.close()

    def exit(self):
        """Close the server socket."""
        logging.info('Close server socket...')

        for in_socket in self._in_sockets:
            self.remove_socket(in_socket)

        self.serversocket.close()


def msg_builder(service_id, msg_id, msg):
    """Returns *msg* as Knut message with header as byte array."""
    data = dict()

    data['serviceId'] = service_id
    data['msgId'] = msg_id
    data['msg'] = msg

    data_str = json.dumps(data)
    msg_size = len(data_str).to_bytes(4, byteorder='big')

    logging.debug(str('Build %i byte long message %s.'
                      % (len(data_str), str(data_str))))

    return msg_size + bytearray(data_str, ENCODING)
