from pytradfri import Gateway
from pytradfri.api.libcoap_api import APIFactory
from knutservices import Light
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
        gateway = Gateway()
        self.device = self.api(gateway.get_device(self.device_id))

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

        # Start the observation first and then the timer to prevent a request
        # timeout.
        self.start_observe()
        observation_timer_thread = threading.Thread(
            target=self.observation_timer,
            name='observation_timer_thread'
        )
        observation_timer_thread.daemon = True
        observation_timer_thread.start()

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
        self.api(self.device.update())
        device_state = self.device.light_control.lights[0]
        light_control = self.device.light_control

        try:
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
            logging.error('pytradfri has a request timeout.')
            self.update_device()

    def observation_timer(self):
        """Restarts the observation of TRÅDFRI devices.

        The observation of the TRÅDFRI devices is restarted every minute to
        ensure the connection.
        """
        while True:
            time.sleep(60)
            self.start_observe()

    def start_observe(self, *args):
        """Observe the TRÅDFRI devices in a new thread.

        If a TRÅDFRI device changes, :meth:`update_backend` will be called
        for that *device* to update the back-end if necessary. If an error
        occurs, this method will be called again to restart the observation
        thread. If no error occurs, the observation ends after one minute.
        """
        # args is needed to be called as fallback by the observer
        # TODO: Check if an other fallback for the observer could restart with
        # a delay the observation.
        try:
            observe_thread = threading.Thread(
                target=self.api,
                name='observe_thread',
                args=(self.device.observe(self.update_backend,
                                          self.start_observe,
                                          duration=60),)
            )
            observe_thread.daemon = True
            observe_thread.start()
            time.sleep(1)  # sleep is needed to start observation task
            logging.debug("Start observing TRADFRI lights.")
        except RuntimeError:
            logging.error("Can't start new thread for observation.")
            self.start_observe()

    def update_backend(self, device):
        """Updates the back-end to the *device* state.

        Updates the back-end to the parsed TRÅDFRI *device*. The update is only
        done, if the id of *device* equals the ``device_id``.
        """
        if device.id != self.device_id:
            return

        logging.debug('Update the TRADFRI light %i in the back-end.' % device.id)
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

        # TODO: add color handling
        self.on_change(self.unique_name)  # trigger on_change to notify listener
