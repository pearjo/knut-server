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
from events import Events
import knut.services
import logging


class Local(Events):
    """Knut local API.

    To interact with the local service, this API has as request handler method
    :meth:`request_handler()`. Using this method, requests can be send to the
    service to get e.g. information about the next sun rise.

    The following message types *msg_id* are supported with the required
    message *msg* and their *response* with its *response_id*:

    .. py:data:: LOCAL_REQUEST
       :value: 0x0001

       Requests local information. The message *msg* can be empty.

    .. py:data:: LOCAL_RESPONSE
       :value: 0x0101

       The local information *response* has the keys the keys ``'isDaylight'``,
       ``'location'``, ``'sunrise'``, ``'sunset'`` and ``'uniqueName'``. For
       example::

          {
              'isDaylight': True,
              'location': 'Hamburg',
              'sunrise': 1589513114.5880833,
              'sunset': 1589483203.418921,
              'uniqueName': 'myLocation'
          }

    """
    NULL = 0x0000
    LOCAL_REQUEST = 0x0001
    LOCAL_RESPONSE = 0x0101

    serviceid = 0x04
    """The local service id."""

    def __init__(self):
        self.local = None
        """The :class:`knut.services.Local` object."""

        self.__events__ = ('on_push')

    def set_local(self, local):
        """Sets :attr:`local` to the *local* object."""
        self.local = local

        # add the notifier method to the local's on_change event
        if callable(self.local.on_change):
            self.local.on_change += self.notifier

    def request_handler(self, msg_id, msg):
        """Returns the tuple (*response_id*, *response*) upon a request.

        The following messages *msg* with their *msg_id* can be send by a client
        and will be handled:

        - :const:`LOCAL_REQUEST`
        """
        response = dict()
        response_id = Local.NULL

        logging.debug('Received local request.')

        if msg_id == Local.LOCAL_REQUEST:
            response_id, response = Local.LOCAL_RESPONSE, self.local.local()

        return response_id, response

    def notifier(self, local):
        self.on_push(Local.serviceid, Local.LOCAL_RESPONSE, local)
