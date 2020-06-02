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
from knut.apis import KnutAPI
import knut.services
import logging


class Local(KnutAPI):
    """This class allows interaction with the local service. The following message
    types are handled by the ``request_handler()`` method of the superclass:

    - ``LOCAL_REQUEST``
    - ``LOCAL_RESPONSE``

    This class can only be connected to one :attr:`local` service by setting it
    using the :meth:`set_local()` method. The ``on_change()`` event of the
    service is connected to the :meth:`notifier()` method, which triggers the
    ``on_push()`` event with a ``LOCAL_RESPONSE``.

    .. py:data:: LOCAL_REQUEST
       :value: 0x0001

       Requests local information. The message *msg* can be empty.

    .. py:data:: LOCAL_RESPONSE
       :value: 0x0101

       The local information *response* has the keys ``'isDaylight'``,
       ``'location'``, ``'sunrise'``, ``'sunset'`` and ``'uniqueName'``. See
       :meth:`knut.services.Local.local()` for more information about the keys
       and their values. Following an example response::

          {
              'isDaylight': True,
              'location': 'Hamburg',
              'sunrise': 1589513114.5880833,
              'sunset': 1589483203.418921,
              'uniqueName': 'myLocation'
          }

    """
    LOCAL_REQUEST = 0x0001
    LOCAL_RESPONSE = 0x0101

    serviceid = 0x04
    """The local service identifier."""

    def __init__(self):
        super(Local, self).__init__()

        self.local = None
        """The :class:`knut.services.Local` object."""

        self.supported = {Local.LOCAL_REQUEST: self.__handle_local_request}

    def __handle_local_request(self, _msg):
        return Local.LOCAL_RESPONSE, self.local.local()

    def set_local(self, local):
        """Sets :attr:`local` to the *local* object."""
        self.local = local

        # add the notifier method to the local's on_change event
        if callable(self.local.on_change):
            self.local.on_change += self.notifier

    def notifier(self, local):
        self.on_push(Local.serviceid, Local.LOCAL_RESPONSE, local)
