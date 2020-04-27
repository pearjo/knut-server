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
from events import Events
import glob
import json
import knut.services
import logging
import os


class Task(Events):
    """Knut task API.

    The task API has the dictionary :attr:`task` as attribute where all tasks
    are stored in. Using :meth:`load_tasks()`, the :attr:`tasks` dictionary can
    be filled with tasks that are stored in a directory :attr:`dir`. Using the
    :meth:`request_handler()`, tasks can be e.g. read or created.

    The following message types *msg_id* are supported with the required
    message *msg* and their *response* with its *response_id*:

    .. py:data:: REMINDER
       :value: 0x0101

       A message that is send as reminder. The seconds before due when the
       reminder is send are defined by :attr:`knut.services.Task.reminder`.
       The message has the following keys:

       - ``'uid'`` unique identifier of the task :attr:`knut.services.Task.uid`
       - ``'reminder'`` the seconds before due at which the reminder is send

       Example of a reminder message::

          {
              'uid': 'f3b14c5e-8458-11ea-9daa-b88a60bd7559',
              'reminder': 3600
          }

    .. py:data:: TASK_REQUEST
       :value: 0x0002

       Requests a task. The *msg* must have the key ``'uid'``.

    .. py:data:: TASK_RESPONSE
       :value: 0x0102

       The task response is the dictionary returned by
       :meth:`knut.services.Task.task()`. If the task response is send from a
       client with an empty ``'uid'``, a new task will be created.

    .. py:data:: ALL_TASKS_REQUEST
       :value: 0x0003

       Requests a list of all tasks. A *msg* is not required and can be empty.

    .. py:data:: ALL_TASKS_RESPONSE
       :value: 0x0103

       The all tasks response is a dictionary with the uids of the tasks as keys
       and the dictionary returned by :meth:`knut.services.Task.task()` as
       values.

    Here's a small example on how to create a new task::

       from knut.apis import Task

       task_api = Task()

       # since no uid is an emtpy string, my_task will be as a new task
       my_task = {
           'assignee': 'John',
           'author': 'Bob',
           'body': 'Can you please get some cheese at the supermarket.',
           'title': 'Buy cheese',
           'uid': ''
       }

       task_api.request_handler(Task.TASK_RESPONSE, my_task)

    Note that there is no directory specified in the example. Therefore, the
    task would not be saved by the :class:`knut.services.Task` service.

    """
    NULL = 0x0000
    REMINDER = 0x0101
    TASK_REQUEST = 0x0002
    TASK_RESPONSE = 0x0102
    ALL_TASKS_REQUEST = 0x0003
    ALL_TASKS_RESPONSE = 0x0103

    serviceid = 0x03
    """The task service id."""

    def __init__(self):
        self.task_dir = str()
        """The directory where the tasks a saved."""
        self.tasks = dict()
        """A dictionary with all back-ends where the keys are the
        :attr:`knut.services.Task.uid` and the values are the corresponding task
        objects :class:`knut.services.Task`
        """

        self.__events__ = ('on_push')

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

                if 'uid' not in data.keys():
                    logging.warning(
                        'Tried to load invalid task \'%s\'.' % task)
                else:
                    logging.debug('Loading task from file \'%s\'...' % task)
                    uid = data['uid']
                    loaded_task = knut.services.Task(uid, task_dir)
                    loaded_task.update_task(data)
                    loaded_task.on_remind += self._reminder
                    self.tasks[uid] = loaded_task

    def request_handler(self, msg_id, msg):
        """Returns the tuple (*response_id*, *response*) upon a request.

        The following messages *msg* with their *msg_id* can be send by a client
        and will be handled:

        - :const:`TASK_REQUEST`
        - :const:`TASK_RESPONSE`
        - :const:`ALL_TASKS_REQUEST`
        """
        response = dict()
        response_id = Task.NULL
        logging.debug('Received a task request.')

        if msg_id == Task.TASK_REQUEST:
            response_id, response = self._handle_task_request(msg)
        elif msg_id == Task.TASK_RESPONSE:
            response_id, response = self._handle_task_response(msg)
        elif msg_id == Task.ALL_TASKS_REQUEST:
            response_id, response = self._handle_all_task_request()

        # check if the response is valid
        response_id = response_id if len(response) > 0 else Task.NULL

        return response_id, response

    def _handle_task_request(self, msg):
        """Returns the tuple (TASK_RESPONSE, *response*).

        Handles a TASK_RESPONSE. The *response* is the dictionary returned by
        :meth:`knut.services.Task.task()`.
        """
        response = dict()
        response_id = Task.NULL

        if 'uid' not in msg.keys:
            logging.warning('Invalid TASK_REQUEST received...')
        else:
            uid = msg['uid']

            if uid not in self.tasks.keys():
                logging.warning('No task with the uid \'%s\' known.' % uid)
            else:
                response = self.tasks[uid].task()
                response_id = Task.TASK_RESPONSE

        return response_id, response

    def _handle_task_response(self, msg):
        """Updates a task and returns the tuple (NULL, dict()).

        The *msg* must be a :const:`TASK_RESPONSE` message.
        """
        response = dict()
        response_id = Task.NULL

        if 'uid' not in msg.keys():
            logging.warning('Invalid TASK_RESPONSE received...')
        else:
            uid = msg['uid']

            if uid not in self.tasks.keys():
                if uid == '' or uid is None:
                    new_task = knut.services.Task(task_dir=self.task_dir)
                    new_task.update_task(msg)
                    self.tasks[new_task.uid] = new_task

                    # send a ALL_TASKS_RESPONSE after adding the new task
                    response_id, response = self._handle_all_task_request()
                else:
                    logging.warning('No task with the uid \'%s\' known.' % uid)
            else:
                self.tasks[uid].update_task(msg)

        return response_id, response

    def _handle_all_task_request(self):
        """Returns the tuple (ALL_TASKS_RESPONSE, *response*).

        The *response* is a dictionary with the :attr:`tasks` keys as keys
        and :meth:`knut.services.Task.task()` as values.
        """
        response = dict()

        for uid, task in self.tasks.items():
            response[uid] = task.task()

        return Task.ALL_TASKS_RESPONSE, response

    def _reminder(self, uid):
        msg = {'uid': uid, 'reminder': self.tasks[uid].reminder}
        self.on_push(Task.serviceid, Task.REMINDER, msg)
