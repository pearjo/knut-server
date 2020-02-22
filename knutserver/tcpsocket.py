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
from socket import socket, AF_INET, SOCK_STREAM, SOL_SOCKET, SO_REUSEADDR
import json
import logging
import threading

encoding = 'UTF-8'


class KnutTcpSocket():
    """The knut TCP socket.

    The IP address and port where the knut socket should be opened are specified
    by *host* and *port*. The maximal amount of data to be received is set by
    the *bufsize* and is on default set to 1024.

    A Knut message is split into the following three parts:

    1. ``msg_size`` : The message size field has the length of 2 bytes
        and represents the number of bytes needed by the message as hex.
        Due to the size of two bytes, a message is limited to a length of 65535
        byte. The byte order of the message size is big endian.
    2. ``service_id`` : The service ID is 1 byte long and as hex
    3. ``msg`` : The message is formatted as JSON and encoded as UTF-8.

    For each received message, a request handler is called to process the
    incoming request by the client and to send a proper response.
    """
    def __init__(self, host='localhost', port=8080, bufsize=1024):
        self.services = dict()
        self.bufsize = bufsize
        self.addr = (host, port)
        self.serversocket = socket(AF_INET, SOCK_STREAM)
        self.serversocket.setsockopt(SOL_SOCKET, SO_REUSEADDR, 1)
        self.serversocket.bind(self.addr)
        self.serversocket.listen(5)
        logging.debug(str('Open server on socket ' + host + ':' + str(port)))
        self.clients = [self.serversocket]

        # start listener
        try:
            listenerThread = threading.Thread(target=self.listener,
                                              name='listenerThread')
            listenerThread.daemon = True
            listenerThread.start()
        except RuntimeError:
            # TODO: kill knut here since nothing makes any sense anymore
            logging.error('Can\'t start new thread for listener.')

    def add_service(self, service):
        """Add a service to the socket.

        The passed *service* is added to the dictionary of services and it's
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
        except queue.Empty:
            logging.debug('Socket output queue for %s is empty.'
                          % str(out_socket.getpeername()))
            self._out_sockets.remove(out_socket)
        else:
            logging.debug('Send %s to %s...' % (next_msg,
                                                str(out_socket.getpeername())))
            out_socket.sendall(next_msg)

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
                ConnectionResetError):
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
        """Send a message to all open sockets.

        The message *msg* needs to be of type ``dict`` and is deserialized
        using the JSON format.
        """
        for client in self.clients:
            if client is not self.serversocket:
                try:
                    logging.debug(str('Send message ' + json.dumps(msg)))
                    byte_msg = self.msg_builder(service_id, msg_id, msg)
                    client.sendall(byte_msg)
                except BrokenPipeError:
                    logging.critical('Connection to client lost.')
                    if client in self.clients:
                        self.clients.remove(client)

    def msg_builder(self, service_id, msg_id, msg):
        """Build a message byte array.

        Build a byte array from the *msg* with the message size field.
        """
        msg_str = json.dumps(msg)
        if len(msg_str) > 65535:
            raise ValueError('message size must be in range(0, 65535)')

        msg_size = len(msg_str).to_bytes(2, byteorder='big')
        service_id = service_id.to_bytes(1, byteorder='big')
        msg_id = msg_id.to_bytes(2, byteorder='big')

        return msg_size + service_id + msg_id + bytearray(msg_str, encoding)

    def exit(self):
        """Close the server socket."""
        logging.info('Close server socket.')
        self.serversocket.close()
