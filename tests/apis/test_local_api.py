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
from knut.apis import Local
import knut.services
import logging
import unittest

logging.disable(logging.CRITICAL)


class TestLocal(unittest.TestCase):
    """Unit tests for the :class:`knut.apis.local.Local` API."""

    def setUp(self):
        local = knut.services.Local('test')
        self.local_api = Local()
        self.local_api.set_local(local)

    def test_local_request(self):
        msgid, msg = self.local_api.request_handler(Local.LOCAL_REQUEST, {})

        self.assertEqual(msgid, Local.LOCAL_RESPONSE)

        keys = set(["id", "isDaylight", "location", "sunrise", "sunset"])

        self.assertEqual(keys, set(msg.keys()))


if __name__ == '__main__':
    unittest.main()
