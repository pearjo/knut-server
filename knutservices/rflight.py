from knutservices import Light
import logging


class RFLight(Light):
    """RF 433 mHz light service.

    This light service is used for lights behind a RF 433 mHz socket.  To work,
    this class needs a transmitter installed at a *gpio*.  Also, the decimal
    *code_on* and *code_off* to switch the socket is needed.
    For more details have a look at `rpi-rf <https://github.com/milaq/rpi-rf>`_.
    """
    def __init__(self, location, unique_name, room, gpio, code_on, code_off):
        super(RFLight, self).__init__(location, unique_name, room)
        self._gpio = gpio
        self._code_on = code_on
        self._code_off = code_off
        self._runnable = False

        try:
            global rpi_rf
            rpi_rf = __import__('rpi_rf')
            self._runnable = True
        except RuntimeError:
            pass

    def status_setter(self, status):
        if self._runnable:
            logging.debug('Enable TX for device \'%s\'...' % self.unique_name)
            device = rpi_rf.RFDevice(self._gpio)
            enabled = device.enable_tx()

            if not enabled:
                logging.error('Failed to enable TX for \'%s\''
                              % self.unique_name)

            if status['state']:
                device.tx_code(self._code_on)
            elif not status['state']:
                device.tx_code(self._code_off)

            device.cleanup()
            del(device)
        else:
            logging.error('\'%s\' needs to run on a Raspberry Pi.'
                          % self.unique_name)

        super(RFLight, self).status_setter(status)
