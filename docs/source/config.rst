.. _config:

*************
Configuration
*************

Knut is configured by a YAML configuration file. Knut objects are defined
directly in the configuration and are initialize from the file. To define a Knut
object in the configuration, the tag ``!knutobject`` is used:

.. code-block:: yaml

   !knutobject
     module: module
     class: Class
     attribute: value
     ...

The keys ``module`` and ``class`` are mandatory and specify the Class and the
module containing it to load. The following keys are the arguments of the
classes ``__init__()`` method. For example, the
:class:`knut.server.KnutTCPServer` would be configured as following:

.. code-block:: yaml

   !knutobject
     module: knut.server
     class: KnutTCPServer
     address: 127.0.0.1
     port: 8080


If no configuration is found, default values are used which are documented in
the following sections.

.. _serverconfig:

server
######

Configuration options to setup the :py:class:`~knut.server.KnutTCPServer`. The
TCP server is bound to the ``address`` on the specified ``port``. The default
values are for the address ``127.0.0.1`` and for the port ``8080``:

.. code-block:: yaml

   server:
     !knutobject
       module: knut.server.tcpserver
       class: KnutTCPServer
       address: 127.0.0.1
       port: 8080

.. _lightconfig:

lights
######

Configures all light back-ends which are subclassed from the
:py:class:`~knut.services.Light` superclass. To add a back-end service, the
back-end object needs to be add to the lights sequence. For example, a
:py:class:`~knut.services.pytradfri.PyTradfriLight` light is add as following:

.. code-block:: yaml

   lights:
     - !knutobject
       module: knut.services.pytradfri
       class: PyTradfriLight
       location: Sideboard
       unique_name: table_living_room
       room: Living Room
       device_id: 65536
       host: 192.168.0.2
       psk_id:
       psk:

On default, no light is configured.

local
#####

Configures the :py:class:`~knut.services.Local` service. The local service has
the configuration keys ``location``, ``longitude``, ``latitude`` and
``elevation``. Note that the degree values are decimal values and the elevation
is in metres.  By default, a local at latitude 0, longitude 0 and elevation 0 is
configured. For example, Hamburg in Germany would be defined as following:

.. code-block:: yaml

   local:
     !knutobject
       module: knut.services
       class: Local
       location: Hamburg
       unique_name: hamburg
       latitude: 53.5506401
       longitude: 9.9917872
       elevation: 18

.. _taskconfig:

task
####

Configuration options for the :py:class:`~knut.services.Task` service. Each task
is written to a file in the directory ``task_dir``. The default directory is
``~/.local/share/knut/tasks``:

.. code-block:: yaml

   task:
     !knutobject
       module: knut.apis
       class: Task
       task_dir: "~/.local/share/knut/tasks"


.. _temperatureconfig:

temperature
###########

Configures all temperature back-ends which are subclassed from the
:py:class:`~knut.services.Temperature` superclass. Each back-end must have a
``unique_name`` which is used as identifier in the Knut system. The key
``location`` defines the location name where the temperature is measured e.g. a
room or city name. The temperature back-ends are added analog to the
``temperature`` sequence as light back-ends are add to the ``lights`` sequence.

For example, a temperature back-end with the unique name ``localWeather`` using
the :py:class:`~knut.services.openweathermap.OpenWeatherMap` service with it's
additional argument ``appid`` is add to the temperature sequence:

.. code-block:: yaml

   temperature:
     - !knutobject
       module: knut.services.openweathermap
       class: OpenWeatherMap
       location: Hamburg
       unique_name: localWeather
       appid:
