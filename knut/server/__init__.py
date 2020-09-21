# -*- coding: utf-8 -*-

# Copyright (C) 2020  Joe Pearson
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.

"""
Knut servers (:mod:`knut.server`)
=================================

.. currentmodule:: knut.server

The server provide the interface for various clients to communicate with the
:class:`knut.apis`.

Each server **must** subclass the :class:`KnutServer` which provides the
interface to the APIs.

The base server and helper classes:

.. autosummary::
   :toctree: generated/

   KnutServer
   tcpserver.KnutTCPRequestHandler

The following servers are provided by this module:

.. autosummary::
   :toctree: generated/

   tcpserver.KnutTCPServer
   websocket.KnutWebSocket
"""

from .knutserver import KnutServer
