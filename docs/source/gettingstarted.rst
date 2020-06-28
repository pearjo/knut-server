.. _gettingstarted:

Getting Started
===============

.. _installing:

Installing
----------

Knut runs using Python 3.7 and to install the Knut server, run first::

   pip install -r requirements.txt

to install all needed packages.

.. note::

   Some services may require additional installation steps. Please follow
   the instructions of the :ref:`services` if an error occurs.

To finally install the Knut server run::

  python setup.py install

.. _howknutworks:

How Knut Works
--------------

Knut has various :ref:`apis` to control e.g. lights or supply temperature data
from various sources. Each API can be connected to multiple service back-ends
which are implementing the actions needed to execute a command.

For example, lets switch a light which is plugged into a RF controllable socket.
The light API :py:class:`~knut.apis.Light` class calls a method of the light
service :py:class:`~knut.services.rflight.RFLight`, which switches the socket
according to a parsed status in a back-end.

.. code-block::
   :linenos:

   from knut.apis import Light
   from knut.services.rflight import RFLight

   # first we define a service back-end using the RFLight module
   rf_light = RFLight('Side Board',  # the location of the light
                      'side_board_lamp',  # the unique name of the light
                      'Living Room',  # the room in which the light is located
                      17,  # the GPIO to which the RF module is connected
                      1328465,  # the code which should be send to switch the light on
                      1328468)  # the code which should be send to switch the light off

   # now we setup the API
   light_api = Light()
   light_api.add_backend(rf_light)

   # lets switch the defined light on
   light_api.request_handler(0x0101,  # the APIs message type to change a light
                             {'uniqueName': 'side_board_lamp', 'state': True})

Now to interact with all APIs from another client program, Knut has a TCP
interface, the :py:class:`~knut.server.KnutTCPServer`. Lets extend the code of
the previous example by the following lines to get working Knut server:

.. code-block::
   :linenos:

   from knut.server import KnutTCPServer
   import threading
   import time

   server = KnutTCPServer(("localhost", 8080))
   server.add_api(light_api)  # our light API from the previous example

   with server:
       server_thread = threading.Thread(target=server.serve_forever)
       server_thread.daemon = True
       server_thread.start()
       time.sleep(60)  # run for one minute

   server.shutdown()

Using ``netcat`` as client, the following request is redirected to the request
handler of the API by the server and we can switch our light:

.. code-block:: bash

   echo -ne '{"serviceid": 2, "msgid": 257, "msg": {"uniqueName": "side_board_lamp", "state": true}}\0' | netcat localhost 8080
