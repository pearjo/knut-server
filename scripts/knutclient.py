#!/usr/bin/env python3
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
import knut.server
import argparse
import socket
import json


def send_request(msg, server):
    """Sends the *msg* to the Knut *server* and prints the response."""
    try:
        msg = json.loads(msg)
        byte_msg = knut.server.tcpsocket.msg_builder(msg['serviceId'],
                                                     msg['msgId'],
                                                     msg['msg'])
        server.sendall(byte_msg)

        msg_size = 0

        while msg_size == 0:
            # use this loop to ignore heartbeats that might get into the loop
            msg_size_bytes = server.recv(4)
            msg_size = int.from_bytes(msg_size_bytes, byteorder='big')

        byte_data = server.recv(msg_size)
        data = json.loads(byte_data)
        print(data)
    except KeyError:
        print('failed to send message')
    except json.decoder.JSONDecodeError:
        print('invalid JSON message')


def main():
    print(
        'knutclient.py  Copyright (C) 2020  Joe Pearson\n'
        'This program comes with ABSOLUTELY NO WARRANTY; for details read LICENSE.\n'
        'This is free software, and you are welcome to redistribute it\n'
        'under certain conditions; read LICENSE for details.\n'
    )

    parser = argparse.ArgumentParser(
        description='Runs a Knut client.'
    )
    parser.add_argument('--host', dest='host',
                        default='127.0.0.1',
                        help='The host address of the Knut server')
    parser.add_argument('--port', dest='port',
                        default=8080,
                        help='The port on which the Knut server is running')

    args = parser.parse_args()

    try:
        server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        server.connect((args.host, int(args.port)))
    except ConnectionRefusedError:
        print('failed to connect to knut')
        return

    while True:
        msg = input('>>> msg: ')
        send_request(msg, server)


if __name__ == '__main__':
    try:
        main()
    except KeyboardInterrupt:
        print('\nCheerio!')
        exit(0)
