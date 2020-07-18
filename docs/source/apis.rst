.. _apiref:

****
APIs
****

.. _knutmsg:

The Knut Message
================

As mentioned in the previous sections, Knut provides various APIs to control
services of different types. For example, to control a light service like the
:py:class:`~knut.services.light.PyTradfriLight`, the corresponding
:py:class:`~knut.apis.Light` API is used.

When running ``knutserver.py``, a TCP server is bound to the configured address
and port (see :ref:`config`). To interact with Knut, a client can send a JSON
message terminated with a null byte ``\0``.

Each message to be send to Knut or send form Knut, must have the following JSON
schema:

.. literalinclude:: ../../etc/knutmsg-schema.json
   :language: json

For example, sending a message via ``netcat`` as client with the Knut server
bound to ``127.0.0.1:8080`` could look like the following:

.. code-block:: bash

   $ echo -ne '{"apiId": 0, "msgId": 0, "msg": {}}\0' | netcat localhost 8080

The key ``apiId`` is an identifier of the API which should be used to process
the message. The ``msgId`` tells the API which message is received and ``msg``
contains finally an individual JSON message which is supported by the API.

The following section describes all available APIs with their supported
messages.

.. _supportedmsg:

Supported Messages
==================

Temperature
-----------

.. data:: apiId
   :value: 1
   :noindex:

.. data:: TEMPERATURE_STATUS_REQUEST
   :value: 1

   Requests the status of a back-end. The message message must have the key
   ``id`` which is the identifier of the requested back-end. For example:

   .. code-block:: json

      {
        "id": "myTemperatureBackend"
      }

.. data:: TEMPERATURE_STATUS_RESPONSE
   :value: 2

   Upon a :const:`TEMPERATURE_STATUS_REQUEST` or change of the back-end is a
   status response send which has the following keys:

   * The identifier ``id`` of the back-end.
   * The ``location`` where the temperature is measured.
   * The ``unit`` in which the temperature value is provided.
   * The ``condition`` at the location as a code point for the `Weather Icons
     <https://erikflowers.github.io/weather-icons/>`_ font.
   * The ``temperature`` value.

   .. code-block:: json

      {
        "id": "myTemperatureBackend",
        "location": "Miami",
        "unit": "°C",
        "condition": "\\uf00d",
        "temperature": 30.1
      }

.. data:: TEMPERATURE_LIST_REQUEST
   :value: 3

   Requests a list of all temperature back-ends with their status. The
   ``msg`` can be an empty.

.. data:: TEMPERATURE_LIST_RESPONSE
   :value: 4

   The temperature list response is similar to the
   :const:`TEMPERATURE_STATUS_RESPONSE`, only with the key ``backends``. The
   value of ``backends`` is an array containing the status response of all known
   back-ends. For example:

   .. code-block:: json

      {
        "backends": [
          {
            "id": "myTemperatureBackend1",
            "location": "Miami",
            "unit": "°C",
            "condition": "\\uf00d",
            "temperature": 30.420000000000009
          },
          {
            "id": "myTemperatureBackend1",
            "location": "Hamburg",
            "unit": "°C",
            "condition": "\\uf008",
            "temperature": 14.240000000000009
          }
        ]
      }

.. data:: TEMPERATURE_HISTORY_REQUEST
   :value: 5

   Request the temperature history of a back-end. The ``msg`` equals the
   :const:`TEMPERATURE_STATUS_REQUEST` message.

.. data:: TEMPERATURE_HISTORY_RESPONSE
   :value: 6

   The response upon a temperature history request has the following keys:

   * The identifier ``id`` of the back-end.
   * The ``temperature`` with an array of temperature values.
   * The ``time`` with an array of UNIX timestamps where the entries correspond
     to the temperature array.

   For example:

   .. code-block:: json

      {
        "id": "myTemperatureBackend",
        "temperature": [
          30.420000000000009,
          32.420000000000009
        ],
        "time": [
          1581863822.2132704,
          1581863882.2132704
        ]
      }

Light
-----

.. data:: apiId
   :value: 2
   :noindex:

.. data:: LIGHT_STATUS_REQUEST
   :value: 1

   Requests the status of a back-end. The message has the key ``id`` with the
   identifier of the back-end as value. The server will respond with a
   :const:`LIGHT_STATUS_RESPONSE`. For example:

   .. code-block:: json

      {
        "id": "myLightBackend"
      }

.. data:: LIGHT_STATUS_RESPONSE
   :value: 2

   The status of a back-end with the following keys:

   * The identifier ``id`` of the back-end.
   * The ``location`` where the light is located inside a room.
   * The ``room`` in which the light is.
   * The ``state`` of the light whether it is on or off as boolean.
   * The ``hasTemperature`` key indicating whether the light has a light
     temperature.
   * The ``hasDimlevel`` key indicating whether the light is dimmable.
   * The ``hasColor`` key indicating whether the light can change its color.
   * The lights ``temperature`` as percentage value where 0 is the coldest and
     100 the warmest color temperature.
   * The ``colorCold`` defining the lights color when the temperature is 0 as
     hex code.
   * The ``colorWarm`` defining the lights color when the temperature is 100 as
     hex code.
   * The ``dimlevel`` in percentage.
   * The light's ``color`` as hex code.

   .. note::

      A client can send the status response to change the state of the light.
      When sending from a client, only the ``id`` key is required and the key of
      which the value should be changed.

   For example a response send by the server:

   .. code-block:: json

      {
        "id": "myLightBackend",
        "location": "Somewhere in a room...",
        "room": "Some room",
        "state": false,
        "hasTemperature": true,
        "hasDimlevel": true,
        "hasColor": false,
        "temperature": 100,
        "colorCold": "#f5faf6",
        "colorWarm": "#efd275",
        "dimlevel": 50,
        "color": ""
      }

   An example response send by a client to switch the light on:

   .. code-block:: json

      {
        "id": "myLightBackend",
        "state": true
      }

.. data:: LIGHTS_REQUEST
   :value: 3

   Requests the status of all back-ends. The message ``msg`` can be empty.

.. data:: LIGHTS_RESPONSE
   :value: 4

   The lights response is similar to the :const:`LIGHT_STATUS_RESPONSE`, only
   with the key ``backends``. The value of ``backends`` is an array containing
   the status response of all known back-ends. For example:

   .. code-block:: json

      {
        "backends": [
          {
            "id": "myLightBackend1",
            "location": "Somewhere in a room...",
            "room": "Some room",
            "state": false,
            "hasTemperature": true,
            "hasDimlevel": true,
            "hasColor": false,
            "temperature": 100,
            "colorCold": "#f5faf6",
            "colorWarm": "#efd275",
            "dimlevel": 50,
            "color": ""
          },
          {
            "id": "myLightBackend1",
            "location": "Somewhere else in a room...",
            "room": "Some other room",
            "state": true,
            "hasTemperature": true,
            "hasDimlevel": true,
            "hasColor": false,
            "temperature": 100,
            "colorCold": "#f5faf6",
            "colorWarm": "#efd275",
            "dimlevel": 50,
            "color": ""
          }
        ]
      }

.. data:: ALL_LIGHTS_REQUEST
   :value: 5

   Requests the combined state of all lights. The message can be empty.

.. data:: ALL_LIGHTS_RESPONSE
   :value: 6

   The response has the key ``state``, where the value *-1* indicates all lights
   off and *1* all lights on. If the value is *0*, neither all lights are on nor
   off.

   .. note::

      If this message is send from a client, only *-1* and *1* can
      be set as value.

   For example:

   .. code-block:: json

      {
        "state": 1
      }

.. data:: ROOMS_LIST_REQUEST
   :value: 7

   Requests a list of all rooms with their state. The message can be empty.

.. data:: ROOMS_LIST_RESPONSE
   :value: 8

   The response has a key ``rooms`` with an array as value containing room state
   objects. A room state object has the following keys:

   * The identifier ``id`` of the room.
   * The room's ``state`` with the possible value *-1*, *0* and *1* analog to
     the :const:`ALL_LIGHTS_RESPONSE`.

   For example:

   .. code-block:: json

      {
        "rooms": [
          {
            "id": "myRoom1",
            "state": -1
          },
          {
            "id": "myRoom2",
            "state": 0
          }
        ]
      }

.. data:: ROOM_REQUEST
   :value: 9

   Requests to set the state for a room. The message has the following keys:

   * The identifier ``id`` of the room.
   * The ``state`` which should be set, where only *-1* and *1* are valid
     values.

   .. code-block:: json

      {
        "id": "myRoom1",
        "state": -1
      }

.. data:: ROOM_RESPONSE
   :value: 10

   The response has the same keys as the :const:`ROOM_REQUEST`, only that the
   value of ``state`` can be also *0*.

Task
----

.. data:: apiId
   :value: 3
   :noindex:

.. data:: REMINDER
   :value: 1

   A message that is send as reminder. It has the following keys:

   - The identifier ``id`` of the task.
   - The ``timeRemaining`` in seconds before due.

   .. code-block:: json

      {
        "id": "f3b14c5e-8458-11ea-9daa-b88a60bd7559",
        "timeRemaining": 3600
      }

.. data:: TASK_REQUEST
   :value: 2

   Requests the task with the identifier ``id``.

   .. code-block:: json

      {
        "id": "f3b14c5e-8458-11ea-9daa-b88a60bd7559"
      }

.. data:: TASK_RESPONSE
   :value: 3

   The task response has the following keys:

   - The identifier ``id`` of the task.
   - The ``assignee`` who has the task assigned.
   - The ``author`` of the task.
   - The task ``description``.
   - The boolean ``done`` which describes whether the task is done or not.
   - The ``due`` time of the task as UNIX timestamp.
   - The UNIX timestamp ``reminder``, when a :const:`REMINDER` should be send.
   - The ``title`` of the task.

   .. note::

      If the task response is send by a client with an empty ``id``, a new task
      will be created.

   .. code-block:: json

      {
        "id": "f3b14c5e-8458-11ea-9daa-b88a60bd7559",
        "assignee": "John",
        "author": "Doug",
        "description": "We need 12 bottles Champagne for the party tonight!",
        "done": false,
        "due": 1577815200,
        "reminder": 1577800800,
        "title": "Drinks for the party"
      }

.. data:: ALL_TASKS_REQUEST
   :value: 4

   Requests a list of all tasks. The message can be empty.

.. data:: ALL_TASKS_RESPONSE
   :value: 5

   The response has the key ``tasks`` with an array as value, containing the
   object of the :const:`TASK_RESPONSE` for each task. For example:

   .. code-block:: json

      {
        "tasks": [
          {
            "id": "f3b14c5e-8458-11ea-9daa-b88a60bd7559",
            "assignee": "John",
            "author": "Doug",
            "description": "We need 12 bottles Champagne for the party tonight!",
            "done": false,
            "due": 1577815200,
            "reminder": 1577800800,
            "title": "Drinks for the party"
          },
          {
            "id": "f3b14c5e-8458-11ea-9daa-va131fassd59",
            "assignee": "John",
            "author": "Doug",
            "description": "Just saw we ran out of cigars!",
            "done": true,
            "due": 1577815200,
            "reminder": 1577800800,
            "title": "Fill up the humidor"
          }
        ]
      }

.. data:: DELETE_TASK_REQUEST
   :value: 6

   Requests to delete a task. The message has the identifier key ``id`` of the
   task which should be deleted.

Local
-----

.. data:: apiId
   :value: 4
   :noindex:

.. _LOCAL_REQUEST:

.. data:: LOCAL_REQUEST
   :value: 1

   Requests local information. The message ``msg`` can be empty.

.. data:: LOCAL_RESPONSE
   :value: 2

   The local information response has the following keys:

   - The identifier ``id`` of the local object.
   - The boolean indicator ``isDaylight`` describing whether the sun has already
     set or not.
   - The ``location`` of the location information.
   - The next ``sunrise`` as UNIX timestamp.
   - The next ``sunset`` as UNIX timestamp.

   For example:

   .. code-block:: json

      {
        "id": "home",
        "isDaylight": true,
        "location": "Beverly Hills",
        "sunrise": 1589513114.5880833,
        "sunset": 1589483203.418921
      }
