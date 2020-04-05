"""
Copyright (C) 2020  Joe Pearson

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.

"""
from knut.services import Light
from pytradfri import Gateway
from pytradfri.api.libcoap_api import APIFactory
import logging
import pytradfri.error
import threading
import time


class PyTradfriLight(Light):
    """PyTradfriLight light service.

    This class uses the `pytradfri <https://github.com/ggravlingen/pytradfri>`_
    package to control an IKEA TRÅDFRI light. A light is located at a
    *location* inside a *room*. It has a *unique_name* for the Knut system and
    a *device_id* inside the TRÅDFRI system. To connect to the TRÅDFRI gateway
    with the known *host* address, a pre-shared key *psk* with its id
    *psk_id* is used. Those are generated as following::

       from pytradfri.api.libcoap_api import APIFactory
       import uuid

       host = ''  # the IP of the gateway
       key = ''  # the 'Security Code' of the Gateway (16 chars)

       psk_id = uuid.uuid4().hex
       api_factory = APIFactory(host=host, psk_id=psk_id)
       psk = api_factory.generate_psk(key)
       print('Generated PSK: ', psk)
       print('Generated PSK_ID: ', psk_id)

    To get the *device_id* of all devices known by the gateway, run the
    following commands with the ``host``, ``psk_id`` and ``psk`` obtained in
    the previous step::

       from pytradfri.api.libcoap_api import APIFactory
       from pytradfri import Gateway

       api = APIFactory(host, psk_id, psk).request
       gateway = Gateway()
       devices_commands = api(gateway.get_devices())
       devices = api(devices_commands)

       for device in devices:
           print('Found device %s with the device_id %i'
                 % (device.name, device.id))

    """
    def __init__(self, location, unique_name, room, device_id, host, psk_id, psk):
        super(PyTradfriLight, self).__init__(location, unique_name, room)

        self.device_id = device_id
        """The TRÅDFRI device id."""

        # set color temperature limits for TRÅDFRI lights
        self.color_cold = '#f5faf6'
        self.color_warm = '#efd275'

        self.api = APIFactory(host, psk_id, psk).request
        self.device = None
        gateway = Gateway()

        while not self.device:
            logging.debug('Try to get device \'%s\'...' % unique_name)
            try:
                self.device = self.api(gateway.get_device(self.device_id))
            except pytradfri.error.RequestTimeout:
                pass
            except FileNotFoundError:
                logging.critical('Failed to load pytradfri service \'%s\'.'
                                 % self.unique_name)
                return

        # get device information
        self.state = self.device.light_control.lights[0].state

        if self.device.light_control.can_set_color:
            self.has_color = True
        else:
            self.has_color = False

        if self.device.light_control.can_set_dimmer:
            self.has_dimlevel = True
            # convert from range [0, 254] to [0, 100]
            dimlevel_hex = self.device.light_control.lights[0].dimmer
            self.dimlevel = round(dimlevel_hex/254*100, 0)
            self.saved_dimlevel = self.dimlevel if self.dimlevel > 0 else 1
        else:
            self.has_dimlevel = False

        if self.device.light_control.can_set_temp:
            self.has_temperature = True
            # get the temperature in mired and convert to range [0, 100]
            temperature_mired = self.device.light_control.lights[0].color_temp
            self.temperature = self.mired_to_precent(temperature_mired)
        else:
            self.has_temperature = False

        # start the TRÅDFRI observation
        observation_thread = threading.Thread(target=self.observation,
                                              name='%s-thread' % unique_name)
        observation_thread.daemon = True
        observation_thread.start()
        logging.info('Initialized TRADFRI device \'%s\'.' % self.unique_name)

    def percent_to_mired(self, value):
        """Return the mired value of *value*.

        Convert the *value* which is in the range [0, 100], where 0 is cold and
        100 warm, to the corresponding mired value.
        """
        if not self.has_temperature:
            raise AttributeError('Light \'%s\' has no temperature'
                                 % self.unique_name)

        diff = (self.device.light_control.max_mireds
                - self.device.light_control.min_mireds)
        value_mired = self.device.light_control.min_mireds + diff*value/100
        return int(round(value_mired, 0))

    def mired_to_precent(self, value):
        """Return the mired *value* in percentage.

        Convert the mired *value* to a percentage value in the range [0, 100],
        where 0 is cold and 100 warm.
        """
        if not self.has_temperature:
            raise AttributeError('Light \'%s\' has no temperature'
                                 % self.unique_name)

        diff = (self.device.light_control.max_mireds
                - self.device.light_control.min_mireds)
        percent = (value - self.device.light_control.min_mireds)/diff*100
        return int(round(percent, 0))

    def status_setter(self, status):
        super(PyTradfriLight, self).status_setter(status)
        self.update_device()

    def update_device(self):
        """Updates the TRÅDFRI device to the back-end."""
        try:
            self.api(self.device.update())
            device_state = self.device.light_control.lights[0]
            light_control = self.device.light_control

            if device_state.state != self.state:
                self.api(light_control.set_state(self.state))

            if self.has_dimlevel:
                dimlevel_hex = int(254*self.dimlevel/100)
                if device_state.dimmer != dimlevel_hex:
                    self.api(light_control.set_dimmer(dimlevel_hex))

            if self.has_temperature:
                temperature_mired = self.percent_to_mired(self.temperature)
                if (device_state.color_temp != temperature_mired):
                    self.api(light_control.set_color_temp(temperature_mired))
        except pytradfri.error.RequestTimeout:
            logging.error('\'%s\' has a request timeout.' % self.unique_name)
            self.update_device()

    def observation(self):
        """Observes the TRÅDFRI light.

        If the observed light changes, :meth:`update_backend` is called.
        """
        thread = None

        def err_callback(err):
            logging.error('Error in TRADFRI observation \'%s\'.'
                          % self.unique_name)

        while True:
            # check if the observation is alive and restart it if not
            if thread and not thread.is_alive():
                logging.debug('Observation of \'%s\' terminated.'
                              % self.unique_name)
                thread = None

            if not thread:
                thread = threading.Thread(
                    target=self.api,
                    name='%s-observation' % self.unique_name,
                    args=(self.device.observe(self.update_backend,
                                              err_callback,
                                              duration=90),)
                )
                thread.daemon = True
                thread.start()
                logging.debug('Started observation of \'%s\'.'
                              % self.unique_name)

            time.sleep(1)

    def update_backend(self, device):
        """Updates the back-end to the *device* state.

        Updates the back-end to the parsed TRÅDFRI *device*. The update is only
        done, if the id of *device* equals the ``device_id``.
        """
        if device.id != self.device_id:
            return

        lock = threading.Lock()
        lock.acquire()

        logging.debug('Update light \'%s\' in the back-end.'
                      % self.unique_name)
        device_state = device.light_control.lights[0]

        # apply new states to back-end
        self.state = device_state.state

        if self.has_dimlevel:
            # a TRÅDFRI light can be switch off while having a dim level greater
            # zero
            dimlevel = round(device_state.dimmer/254*100, 0)
            self.dimlevel = dimlevel if self.state else 0

            if dimlevel > 0:
                self.saved_dimlevel = dimlevel

        if self.has_temperature:
            self.temperature = self.mired_to_precent(device_state.color_temp)

        lock.release()

        # TODO: add color handling
        self.on_change(self.unique_name)  # trigger on_change to notify listener
