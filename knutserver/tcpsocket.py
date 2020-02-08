import logging
import threading
import json
from socket import socket, AF_INET, SOCK_STREAM, SOL_SOCKET, SO_REUSEADDR

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
        """Listen for connections.

        If a connection is established, the client socket is append to the list
        of clients and a handler thread is started.

        """
        while True:
            logging.debug('Server is listening for connections')
            clientsocket, clientaddr = self.serversocket.accept()
            self.clients.append(clientsocket)

            # start handler thread
            try:
                handler_thread = threading.Thread(
                    target=self.handler,
                    name='handler_thread',
                    args=(clientsocket, clientaddr)
                )
                handler_thread.daemon = True
                handler_thread.start()
            except RuntimeError:
                logging.error('Can\'t start thread for message handler.')

    def handler(self, clientsocket, clientaddr):
        """Handle client requests.

        The handler listens for connections on the *clientsocket* with the IP
        address *clientaddr*. If data are received, they will be serialized to
        the JSON format. Those data in JSON format are then parsed to the
        :meth:`request_handler` and the returned response is send back to the
        client.
        """
        logging.debug(str('Accepted connection from '
                          + clientaddr[0]
                          + ':'
                          + str(clientaddr[1])))
        byte_data = bytearray()
        byte_response = bytearray()
        data = dict()

        while True:
            try:
                # read the first to message size bytes to check if connection
                # is open
                msg_size_bytes = clientsocket.recv(2)
                if not msg_size_bytes:
                    break

                msg_size = int.from_bytes(msg_size_bytes, byteorder='big')
                service_id = int.from_bytes(clientsocket.recv(1),
                                            byteorder='big')
                msg_id = int.from_bytes(clientsocket.recv(2),
                                        byteorder='big')

                if msg_size > 0:
                    byte_data = clientsocket.recv(msg_size)
                    data = json.loads(byte_data, encoding=encoding)
                    logging.debug(str('Received ' + str(data)))
            except (json.decoder.JSONDecodeError, ValueError,
                    ConnectionResetError):
                logging.critical('No valid message received.')

            response_id, response = self.request_handler(service_id, msg_id,
                                                         data)

            if response_id > 0:
                byte_response = self.msg_builder(service_id, response_id,
                                                 response)
                try:
                    logging.debug(str('Send response '
                                      + json.dumps(response)))
                    clientsocket.send(byte_response)
                except BrokenPipeError:
                    logging.critical('Connection to client lost.')
                    break

        logging.debug(str('Close connection to '
                          + clientaddr[0]
                          + ':'
                          + str(clientaddr[1])))
        if clientsocket in self.clients:
            self.clients.remove(clientsocket)
        clientsocket.close()

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

    def send(self, service_id: int, msg_id: int, msg: dict) -> None:
        """Send a message to all open sockets.

        The message *msg* needs to be of type ``dict`` and is deserialized
        using the JSON format.
        """
        for client in self.clients:
            if client is not self.serversocket:
                try:
                    logging.debug(str('Send message ' + json.dumps(msg)))
                    byte_msg = self.msg_builder(service_id, msg_id, msg)
                    client.send(byte_msg)
                except BrokenPipeError:
                    logging.critical('Connection to client lost.')
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
