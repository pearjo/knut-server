.. _knutservices:

Knut Services
=============

Base Services
-------------

The following services are the base services which are inherited by the custom
services. Each custom services can then extend the functionality of the base
service while maintaining its core functionality.

.. autoclass:: knutservices.Light
   :members:

.. autoclass:: knutservices.Temperature
   :members:

Custom Services
---------------

.. autoclass:: knutservices.pytradfri.PyTradfriLight
   :members:

.. autoclass:: knutservices.rflight.RFLight
   :members:

.. autoclass:: knutservices.openweathermap.OpenWeatherMap
   :members:
