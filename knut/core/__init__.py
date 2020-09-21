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
Core functionality of Knut (:mod:`knut.core`)
=============================================

.. currentmodule:: knut.core

This module provide core functionalities which allow subclassed objects
configuration and integration into Knut.

.. autosummary::
   :toctree: generated/

   KnutConfig
   KnutObject
"""

from .base import KnutObject
from .config import KnutConfig
