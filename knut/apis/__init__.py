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
Knut APIs (:mod:`knut.apis`)
============================

.. currentmodule:: knut.apis

This module provide the APIs to control Knut's :mod:`~knut.services`.
Each API has a set of commands and call-back function assigned to them which are
executed if a :mod:`~knut.server` receives a valid request.

Each API should subclass the base API :class:`KnutAPI` as this class implements
the call-back functionality using a request handler.

.. autosummary::
   :toctree: generated/

   KnutAPI

The following APIs are available:

.. autosummary::
   :toctree: generated/

   Light
   Local
   Task
   Temperature
"""

from .knutapi import KnutAPI
from .light import Light
from .local import Local
from .task import Task
from .temperature import Temperature
