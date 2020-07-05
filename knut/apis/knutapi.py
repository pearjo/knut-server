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
from typing import Tuple
import logging


class KnutAPI(Events):
    """This class is used to provide the basic Knut's API. It must be subclassed
    by each service specific API.

    The :meth:`request_handler()` builds the core for communicate to services
    handled by the API. It handles a request message and calls a callback
    function for each message type. The supported message types with their
    callback functions need to be registered in the :attr:`supported`
    dictionary.

    Each callback function must take the message dictionary as argument and must
    return a tuple with the response message type and the response message
    dictionary.

    For example, lets add the message type ``FOO_PRINT = 0x0001`` with its
    callback function ``handle_foo()``. The example message handler should
    print a *text* that is passed by the message and return a ``NULL`` message.
    Therefore, the message of type ``FOO_PRINT`` in this example shall contain
    the key ``'text'``. Lets first define a :class:`Foo` object which
    inherits the :class:`KnutAPI`::

       from knut.apis import KnutAPI


       class Foo(KnutAPI):
           apiid = 0x01
           FOO_PRINT = 0x0001

           def __init__(self):
               super(Foo, self).__init__()
               self.supported = {Foo.FOO_PRINT: self.handle_foo}

           def handle_foo(self, msg):
               print(msg['text'])
               return Foo.NULL, {}

    With that class defined, our example message would be handled as following::

       >>> foo = Foo()
       >>> foo.request_handler(0x0001, {'text': 'bar'})
       bar
       (0, {})

    """
    NULL = 0x0000
    """The null message indicates that no further action is going to happen."""

    apiid = int()
    """The API identifier."""

    def __init__(self) -> None:
        self.__events__ = ('on_push')

        self.supported = dict()
        """A dictionary with all supported message types as keys and their
        callback functions as values ``{msg_id: callback}``."""

    def request_handler(self, msg_id: int, msg: dict) -> Tuple[int, dict]:
        """Returns the tuple (*response_id*, *response*) upon a request.

        Handles the request *msg* of type *msg_id* and returns a corresponding
        *response* of type *response_id*. If either no valid request *msg* was
        passed, or the request does not expect any response, the response
        ``(0x0000, {})`` is returned.

        This method checks the :attr:`supported` dictionary and calls the
        corresponding callback function if found.
        """
        response_id, response = KnutAPI.NULL, dict()

        if msg_id != KnutAPI.NULL:
            try:
                response_id, response = self.supported[msg_id](msg)
            except KeyError:
                logging.error('Unsupported msg_id for API \'{}\': {}'
                              .format(self.apiid, msg_id))
            except TypeError as e:
                logging.error('Invalid callback function for '
                              'msg_id \'{}\' and API \'{}\': {}'
                              .format(msg_id, self.apiid, e))

        response_id = response_id if len(response) > 0 else KnutAPI.NULL

        return response_id, response
