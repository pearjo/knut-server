.. highlight:: sh

Knut: your humble server!
=========================

What is Knut?
-------------

Knut is a happy penguin to help organize your home. 🐧

Ok... What is Knut again? It's a smart home assistant with a server at it's core
which is connected to various :ref:`apis`. Via a JSON formatted message and the
TCP server, clients can interact with the various APIs. They are designed in
such way, that they can be extended modular. Each API is then connected with
:py:mod:`knut.services` which do some work like switching a light or providing
data to the API.

Run Knut
--------

With Knut and it's requirements installed, you can take the server for a test
run by running the ``knutserver`` with the example configuration::

   knutserver -f etc/example.yml

Now you should be ready to write your own :ref:`config` and have fun using Knut.

More
----

To learn more about Knut and how it works, have a look at the
:ref:`gettingstarted` chapter. For the code documentation see :ref:`reference`.

Checkout the `Knut cross-platform app <https://github.com/pearjo/knut-app>`_
which provides a user front-end to the Knut server.

.. toctree::
   :maxdepth: 2

   guide
   reference
