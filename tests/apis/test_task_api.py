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
from knut.apis import Task
import knut.services
import logging
import unittest

logging.disable(logging.CRITICAL)


class TestTask(unittest.TestCase):
    """Unit tests for the :class:`knut.apis.task.Task` API."""

    def setUp(self):
        self.task_api = Task('tasks')

        # add a new task
        self.task = knut.services.Task('test')
        self.task.update_task(
            {
                'assignee': 'Assignee',
                'author': 'Author',
                'description': 'Description',
                'done': False,
                'due': 0,
                'reminder': 0,
                'title': 'Request Test'
            }
        )

        self.task_api.tasks = {self.task.uid: self.task}

    def test_task_request(self):
        msgid, msg = self.task_api.request_handler(Task.TASK_REQUEST,
                                                   {'id': self.task.uid})
        self.assertEqual(msgid, Task.TASK_RESPONSE)
        self.assertEqual(msg, self.task.task())

    def test_task_response(self):
        new_task = {
            'id': self.task.uid,
            'assignee': 'Assignee',
            'author': 'Author',
            'description': 'Description',
            'done': True,
            'due': 0,
            'reminder': 0,
            'title': 'Response Test'
        }

        msgid, msg = self.task_api.request_handler(Task.TASK_RESPONSE,
                                                   new_task)

        self.assertEqual(self.task_api.tasks[self.task.uid].task(), new_task)

    def test_all_tasks_request(self):
        msgid, msg = self.task_api.request_handler(Task.ALL_TASKS_REQUEST, {})

        tasks = list()

        for task in self.task_api.tasks.values():
            tasks.append(task.task())

        self.assertEqual(msgid, Task.ALL_TASKS_RESPONSE)
        self.assertEqual(msg['tasks'], tasks)

    def test_delete_task_request(self):
        self.task_api.request_handler(Task.DELETE_TASK_REQUEST,
                                      {'id': self.task.uid})

        self.assertEqual(self.task_api.tasks, {})


if __name__ == '__main__':
    unittest.main()
