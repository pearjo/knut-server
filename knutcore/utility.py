import signal
import logging


class KnutUtility():
    def __init__(self):
        self.interrupt = False
        signal.signal(signal.SIGINT, self.signal_handler)

    def signal_handler(self, sigint: int, frame):
        """Handle a system signal.

        :param sigint: The signal number.
        :param frame: The interrupted stack frame.
        """
        logging.debug('System signal received and interrupt is set')
        self.interrupt = True
