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
Knut services (:mod:`knut.services`)
====================================

.. currentmodule:: knut.services

A Knut service provide data and implement functionalities which can be used by
the APIs. For example, to switch a light, a service provide commands which
are used by the API to switch the light on or off and to get e.g. status
information of the light.

.. autosummary::
   :toctree: generated/

   local.Local
   task.Task

.. note::

   Each light service **must** subclass the base class :class:`~light.Light`.

.. autosummary::
   :toctree: generated/

   light.Light
   light.dummylight.DummyLight
   light.pytradfri.PyTradfriLight
   light.rflight.RFLight
   light.tasmota_light.TasmotaLight
   light.wlansocket.WlanSocket

.. note::

   Each temperature service **must** subclass the base class
   :class:`temperature.Temperature`.

.. autosummary::
   :toctree: generated/

   temperature.Temperature
   temperature.dummytemperature.DummyTemperature
   temperature.openweathermap.OpenWeatherMap
"""
