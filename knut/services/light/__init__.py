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

"""Knut light services.

This module provides the Knut light services to be used with the
:class:`~knut.apis.Light` API.

.. note::

   All light services must subclass the base class
   :class:`knut.services.light.Light`.

"""
from .light import *  # base class

from .dummylight import *
from .pytradfri import *
from .rflight import *
