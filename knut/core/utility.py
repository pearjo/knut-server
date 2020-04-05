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
import logging
import signal


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
