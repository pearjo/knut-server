.. _apisandservices:

APIs & Services
===============

Each Knut service has a corresponding API to control it. To integrate into the
Knut system, each service and API is subclassed from a base class to provide all
mechanisms required in the Knut system to the respective service or API. See
:ref:`baseclasses` for more details about the base classes. To following pages
document the various APIs and the services which can be connected to those APIs.

.. toctree::
   :maxdepth: 1
   :caption: Available APIs & Services:

   temperature
   light
   task
   local

.. _baseclasses:

Base Classes
------------

The following classes are the superclasses for all APIs and services. They must
be subclassed by each API and service object.

.. autoclass:: knut.apis.KnutAPI
   :members:
