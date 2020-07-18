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
from .knutapi import KnutAPI
import glob
import json
import knut.services
import logging
import os


class Task(KnutAPI):
    """This class allows interaction with the task service. The following message
    types are handled by the ``request_handler()`` of the superclass:

    - :const:`TASK_REQUEST`
    - :const:`TASK_RESPONSE`
    - :const:`ALL_TASKS_REQUEST`
    - :const:`DELETE_TASK_REQUEST`

    The dictionary :attr:`tasks` stores all tasks. Using :meth:`load_tasks()`,
    the dictionary can be filled with tasks that are stored in a local directory
    :attr:`task_dir`.

    Here's a small example on how to create a new task::

       >>> from knut.apis import Task

       >>> task_api = Task()

       >>> # since no id is an emtpy string, my_task will be as a new task
       >>> my_task = {
       >>>     'assignee': 'John',
       >>>     'author': 'Bob',
       >>>     'body': 'Can you please get some cheese at the supermarket.',
       >>>     'title': 'Buy cheese',
       >>>     'id': ''
       >>> }

       >>> task_api.request_handler(Task.TASK_RESPONSE, my_task)

    In the example is no directory specified. Therefore, the task would not be
    saved by the task service.

    """
    REMINDER = 1
    TASK_REQUEST = 2
    TASK_RESPONSE = 3
    ALL_TASKS_REQUEST = 4
    ALL_TASKS_RESPONSE = 5
    DELETE_TASK_REQUEST = 6

    apiid = 3

    def __init__(self, task_dir="~/.local/share/knut/tasks"):
        super(Task, self).__init__()

        self.supported = {
            Task.TASK_REQUEST: self.__handle_task_request,
            Task.TASK_RESPONSE: self.__handle_task_response,
            Task.ALL_TASKS_REQUEST: self.__handle_all_task_request,
            Task.DELETE_TASK_REQUEST: self.__handle_delete_task_request
        }

        self.task_dir = task_dir
        """The directory where the tasks are saved."""

        self.tasks = dict()
        """A dictionary with all back-ends.

        The keys are the task identifier and the values are the corresponding
        :class:`~knut.services.Task` objects.
        """

    def load_tasks(self, task_dir=None):
        """Load all tasks saved in the directory *task_dir*.

        If *task_dir* is not provided, the tasks from the directory
        :attr:`task_dir` are loaded instead.
        """
        if not task_dir:
            task_dir = self.task_dir

        task_dir = os.path.expanduser(task_dir)
        tasks = glob.glob(os.path.join(task_dir, '*.json'))

        logging.debug('Load tasks from \'%s\'...' % task_dir)

        for task in tasks:
            with open(task) as f:
                data = json.load(f)

                if 'id' not in data.keys():
                    logging.warning(
                        'Tried to load invalid task \'%s\'.' % task)
                else:
                    logging.debug('Loading task from file \'%s\'...' % task)
                    uid = data['id']
                    loaded_task = knut.services.Task(uid, task_dir)
                    loaded_task.update_task(data)
                    loaded_task.on_remind += self.__reminder
                    self.tasks[uid] = loaded_task

    def __handle_task_request(self, msg):
        response = dict()
        response_id = Task.NULL

        if 'id' not in msg.keys():
            logging.warning('Invalid TASK_REQUEST received...')
        else:
            uid = msg['id']

            if uid not in self.tasks.keys():
                logging.warning('No task with the uid \'%s\' known.' % uid)
            else:
                response = self.tasks[uid].task()
                response_id = Task.TASK_RESPONSE

        return response_id, response

    def __handle_task_response(self, msg):
        response = dict()
        response_id = Task.NULL

        if 'id' not in msg.keys():
            logging.warning('Invalid TASK_RESPONSE received...')
        else:
            uid = msg['id']

            if uid not in self.tasks.keys():
                if uid == '' or uid is None:
                    new_task = knut.services.Task(task_dir=self.task_dir)
                    new_task.update_task(msg)
                    new_task.on_remind += self.__reminder
                    self.tasks[new_task.uid] = new_task

                    # send a ALL_TASKS_RESPONSE after adding the new task
                    response_id, response = self.__handle_all_task_request(msg)
                else:
                    logging.warning('No task with the uid \'%s\' known.' % uid)
            else:
                self.tasks[uid].update_task(msg)
                response_id, response = (Task.TASK_RESPONSE,
                                         self.tasks[uid].task())

        if response_id > 0:
            self.on_push(Task.apiid, response_id, response)

        return Task.NULL, dict()

    def __handle_all_task_request(self, _msg):
        """Returns the tuple (ALL_TASKS_RESPONSE, *response*).

        The *response* dictionary has the keys of :ref:`ALL_TASKS_RESPONSE`.
        """
        tasks = list()

        for uid, task in self.tasks.items():
            tasks.append(task.task())

        response = {'tasks': tasks}

        return Task.ALL_TASKS_RESPONSE, response

    def __handle_delete_task_request(self, msg):
        if 'id' not in msg.keys():
            logging.warning('Invalid DELETE_TASK_REQUEST received...')
        elif msg['id'] in self.tasks:
            uid = msg['id']
            self.tasks[uid].delete_task()
            del self.tasks[uid]
            # notify all clients about the changes
            response_id, response = self.__handle_all_task_request(msg)
            self.on_push(Task.apiid, response_id, response)
        else:
            logging.warning(
                'Can\'t delete unknown task \'%s\'...' % msg['id'])

        return Task.NULL, dict()

    def __reminder(self, uid):
        logging.debug('Push reminder for \'%s\'...' % uid)
        msg = {'id': uid, 'reminder': self.tasks[uid].reminder}
        self.on_push(Task.apiid, Task.REMINDER, msg)
