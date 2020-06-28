.. _services:

********
Services
********

Each of the Knut :ref:`apis` controls a dedicated service. Some APIs have
several services e.g. the light API. See the API reference to check if a service
needs to be subclassed from a superclass.

.. autosummary::
   :toctree: generated

   knut.services.light
   knut.services.Local
   knut.services.temperature
   knut.services.Task
