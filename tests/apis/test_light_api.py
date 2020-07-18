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
from knut.apis import Light
import knut.services.light
import logging
import unittest

logging.disable(logging.CRITICAL)


class TestLight(unittest.TestCase):
    """Test knut's light API."""

    def setUp(self):
        self.service = knut.services.light.Light('location', 'id', 'room')
        self.light_api = Light()
        self.light_api.add_backend(self.service)

    def test_light_status_request(self):
        """Test the LIGHT_STATUS_REQUEST."""
        msgid, msg = self.light_api.request_handler(Light.LIGHT_STATUS_REQUEST,
                                                    {'id': self.service.uid})

        self.assertEqual(msgid, Light.LIGHT_STATUS_RESPONSE)
        self.assertEqual(msg, self.service.status())

    def test_light_status_response(self):
        """Test the LIGHT_STATUS_RESPONSE."""
        response_msg = {'id': self.service.uid,
                        'state': not self.service.state}
        msgid, msg = self.light_api.request_handler(Light.LIGHT_STATUS_RESPONSE,
                                                    response_msg)

        self.assertEqual(msgid, Light.LIGHT_STATUS_RESPONSE)
        self.assertEqual(response_msg['state'], msg['state'])


if __name__ == '__main__':
    unittest.main()
