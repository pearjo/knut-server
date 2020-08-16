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
import knut.apis
import logging


class KnutServer():
    """Base class for Knut servers.

    .. note::

       Each subclass must implement :meth:`knut_serve_forever()`.

    """

    def __init__(self, address: str, port: int):
        """A server which binds to the *address* and listens on the *port* for
        requests.
        """
        self.address = address
        """The address to which the server is bound."""

        self.port = port
        """The port on which the server listens."""

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
        else:
            logging.debug('Add api to server: {}'.format(api))

        apiid = api.apiid
        self.apis[apiid] = api

    def knut_serve_forever(self):
        """The main event loop of the server.

        .. warning::

           Must be implemented by a subclass.

        """
        raise AttributeError("'knut_serve_forever' method of server "
                             "not implemented.")
