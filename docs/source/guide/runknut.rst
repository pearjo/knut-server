.. highlight:: sh

Run Knut
========

Command line
------------

When invoking Knut, you may specify any of these options::

   knutserver [-v] [-f FILE]

.. _using-on-interface-options:

Options
~~~~~~~

.. cmdoption:: -h
               --help

   Print a short description of all command line options.

.. cmdoption:: -v
               --verbose

   Set the log level to `DEBUG` to make Knut more talkative. See also
   :envvar:`KNUTDEBUG`.

.. cmdoption:: -f FILE
               --file=FILE

   Load the configuration from the *FILE*. If no file is specified, Knut will
   try to find the configuration ``/etc/knutserver.yml``.

   .. seealso:: :ref:`config` for details about the configuration file.

Debug variables
---------------

.. envvar:: KNUTDEBUG

   Change the log level of the Knut server. By default, the log level
   is `INFO`. Available options are `DEBUG`, `INFO`, `WARNING`,
   `ERROR` and `CRITICAL`.
