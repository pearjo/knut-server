*************************
Knut: your humble server!
*************************

What is Knut?
#############

Knut is a happy penguin to help organize your home. 🐧🥰

Ok... More precise, the Knut server provides APIs for various purposes such as
switching smart lights, providing climate date or a task service. The server is
written in Python and services for the various APIs can be extended modular.

With Knut and it's requirements installed, you can take the server for a test
run with the example configuration::

   knutserver.py --conf=etc/knutserver.yml

Now you should be ready to write your own configuration and have fun using Knut.
To learn more about Knut and how it works, have a look at the
:ref:`gettingstarted` chapter. For the code documentation see :ref:`knutserver`
and :ref:`apisandservices`.

.. note::

   Checkout the `Knut cross-platform app <https://github.com/pearjo/knut-app>`_
   which provides a user front-end to the Knut server.

.. toctree::
   :maxdepth: 2

   gettingstarted
   config
   reference/reference.rst
