# -*- coding: utf-8 -*-

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

from .knutapi import KnutAPI


class Local(KnutAPI):
    """Interact with the local service.

    This class extends the :class:`KnutAPI` to handle the following requests::

    - :const:`LOCAL_REQUEST`
    - :const:`LOCAL_RESPONSE`

    Only one :attr:`local` service can be connected by setting it using
    :meth:`set_local()`. The ``on_change()`` event of the service is connected
    to the :meth:`notifier()` method, which triggers the ``on_push()`` event
    with a :const:`LOCAL_RESPONSE`.
    """

    LOCAL_REQUEST = 1
    LOCAL_RESPONSE = 2

    apiid = 4

    def __init__(self):
        super(Local, self).__init__()

        self.local = None
        """The :class:`~knut.services.Local` object."""

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
        self.on_push(Local.apiid, Local.LOCAL_RESPONSE, local)
