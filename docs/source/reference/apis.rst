.. _apis:

****
APIs
****

This page gives an overview of all available Knut APIs.

.. note::

   Each API must subclass the :py:class:`~knut.apis.KnutAPI` superclass.

.. autosummary::
   :toctree: generated

   knut.apis.Light
   knut.apis.Local
   knut.apis.Task
   knut.apis.Temperature

.. _apibaseclass:

Base Class
==========

The following class is the superclass for all APIs. It must
be subclassed by each API object.

.. autoclass:: knut.apis.KnutAPI
   :members:
