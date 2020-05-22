.. Knut Server documentation master file, created by
   sphinx-quickstart on Sat Dec 21 20:05:33 2019.
   You can adapt this file completely to your liking, but it should at least
   contain the root `toctree` directive.

Knut: your friendly penguin!
============================

What is Knut?
-------------

Knut is a friendly penguin to help organize your home.

To be more precise, the Knut server provides APIs for various purposes such as
switching smart lights, providing climate date or a task service. The server is
written in Python and services for the various APIs can be extended modular.

With Knut and it's requirements installed, you can run the server for example
with the example configuration::

   knutserver.py --conf=etc/knutserver.yml

To learn more about Knut and how it works, have a look at the
:ref:`gettingstarted` chapter. For the code documentation see :ref:`knutserver`,
:ref:`knutapis` and :ref:`knutservices`.

.. note::

   Checkout the `Knut cross-platform app <https://github.com/pearjo/knut-app>`_
   which provides a user front-end to the Knut server.

.. toctree::
   :maxdepth: 2
   :caption: Contents

   gettingstarted
   reference/knutserver
   reference/knutapis
   reference/knutservices
