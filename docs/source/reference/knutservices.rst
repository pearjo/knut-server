.. _knutservices:

Knut Services
=============

Base Services
-------------

The following services are the base services which are inherited by the custom
services. Each custom services can then extend the functionality of the base
service while maintaining its core functionality.

.. autoclass:: knut.services.Light
   :members:

.. autoclass:: knut.services.Local
   :members:

.. autoclass:: knut.services.Task
   :members:

.. autoclass:: knut.services.Temperature
   :members:

Custom Services
---------------

.. autoclass:: knut.services.pytradfri.PyTradfriLight
   :members:

.. autoclass:: knut.services.rflight.RFLight
   :members:

.. autoclass:: knut.services.openweathermap.OpenWeatherMap
   :members:
