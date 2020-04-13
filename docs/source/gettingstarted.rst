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
   the instructions of the :ref:`knutservices` if an error occurs.

To finally install the Knut server run::

  python setup.py install

.. _howknutworks:

How Knut Works
--------------

Knut has various :ref:`knutapis` to control e.g. lights or supply temperature
data from various sources. Each API communicates with :ref:`knutservices` which
are implementing the actions needed to execute a command.

For example, lets switch a light which is plugged into a RF controllable socket.
The light API :py:class:`knut.apis.Light` class calls a method of the light
service :py:class:`knut.services.rflight.RFLight`, which switches the socket
according to a parsed status in a back-end.

Clients can send requests to the server using the protocol specified in
:py:class:`knut.server.KnutTcpSocket`. The payload is API specific. Therefore
have a look at the specific :ref:`knutapis` to see what payloads are requested
for the individual messages. The information flows as following:

.. graphviz::

   digraph flow {
      "Client";

      subgraph request {
         "Server" -> "API" -> "Service";
         label = "Request";
      }

      subgraph response {
         "Service" -> "API" -> "Server";
         label = "Request";
      }

      "Client" -> "Server";
      "Server" -> "Client";
      "Service" -> "Back-End";
      "Back-End" -> "Service";

      "Back-End";
   }

.. _howtoservice:

How to Write a Knut Service
---------------------------

As described in :ref:`howknutworks`, a service is called by an API to do some
stuff. This stuff can be switching a light or returning the current temperature
at some location. A service should therefore represent and implement the
functionality of an object. If something changes in the service, it can notify
it's controlling API by an event.

.. _howtoapi:

How to Write a Knut API
-----------------------

To control a Knut service, we use a Knut API. Each API **must have the following
three things implemented**:

1. An attribute called :py:attr:`serviceid` which is an integer value and
   representing the ID of the service.
2. A callable :py:meth:`on_push()` which needs to be called from within the API
   with a message ID *msg_id* and a message *msg* as following::

      self.on_push(self.serviceid, msg_id, msg)

3. A callable :py:meth:`request_handler()` which is called by the server to
   handle a request. It must take the positional arguments *msg_id* and *msg*
   and returns the tuple (*response_id*, *response*)::

      def request_handler(self, msg_id, msg):
          ...
          return response_id, response
