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
from knut.apis import KnutAPI
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

    The dictionary :attr:`task` stores all tasks. Using :meth:`load_tasks()`,
    the dictionary can be filled with tasks that are stored in a local directory
    :attr:`task_dir`.

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

       Requests the task matching an identifier passed by the message. The key
       ``'uid'`` is required.

    .. py:data:: TASK_RESPONSE
       :value: 0x0102

       The task response is the dictionary returned by
       :meth:`knut.services.Task.task()`. If the task response is received by
       the request handler with an empty ``'uid'``, a new task will be created.

    .. py:data:: ALL_TASKS_REQUEST
       :value: 0x0003

       Requests a list of all tasks. No message is required.

    .. py:data:: ALL_TASKS_RESPONSE
       :value: 0x0103

       The response is a dictionary with the uids of all tasks as keys
       and the dictionary returned by :meth:`knut.services.Task.task()` as
       values.

    .. py:data:: DELETE_TASK_REQUEST
       :value: 0x0004

       Requests to delete a task. The message requires the key ``'uid'`` with
       the identifier of the task which should be deleted. For example, to
       delete the task of the :const:`REMINDER` example, the message should like
       the following::

          {'uid': 'f3b14c5e-8458-11ea-9daa-b88a60bd7559'}

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
    task would not be saved by the task service.

    """
    REMINDER = 0x0101
    TASK_REQUEST = 0x0002
    TASK_RESPONSE = 0x0102
    ALL_TASKS_REQUEST = 0x0003
    ALL_TASKS_RESPONSE = 0x0103
    DELETE_TASK_REQUEST = 0x0004

    serviceid = 0x03
    """The task service identifier."""

    def __init__(self, task_dir="~/.local/share/knut/tasks"):
        super(Task, self).__init__()

        self.supported = {
            Task.TASK_REQUEST: self.__handle_task_request,
            Task.TASK_RESPONSE: self.__handle_task_response,
            Task.ALL_TASKS_REQUEST: self.__handle_all_task_request,
            Task.DELETE_TASK_REQUEST: self.__handle_delete_task_request
        }

        self.task_dir = task_dir
        """The directory where the tasks a saved."""

        self.tasks = dict()
        """A dictionary with all back-ends where the keys are the
        :attr:`knut.services.Task.uid` and the values are the corresponding task
        objects :class:`knut.services.Task`
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

                if 'uid' not in data.keys():
                    logging.warning(
                        'Tried to load invalid task \'%s\'.' % task)
                else:
                    logging.debug('Loading task from file \'%s\'...' % task)
                    uid = data['uid']
                    loaded_task = knut.services.Task(uid, task_dir)
                    loaded_task.update_task(data)
                    loaded_task.on_remind += self.__reminder
                    self.tasks[uid] = loaded_task

    def __handle_task_request(self, msg):
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

    def __handle_task_response(self, msg):
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
            self.on_push(Task.serviceid, response_id, response)

        return Task.NULL, dict()

    def __handle_all_task_request(self, _msg):
        """Returns the tuple (ALL_TASKS_RESPONSE, *response*).

        The *response* is a dictionary with the :attr:`tasks` keys as keys
        and :meth:`knut.services.Task.task()` as values.
        """
        response = dict()

        for uid, task in self.tasks.items():
            response[uid] = task.task()

        return Task.ALL_TASKS_RESPONSE, response

    def __handle_delete_task_request(self, msg):
        if 'uid' not in msg.keys():
            logging.warning('Invalid DELETE_TASK_REQUEST received...')
        elif msg['uid'] in self.tasks:
            uid = msg['uid']
            self.tasks[uid].delete_task()
            del self.tasks[uid]
            # notify all clients about the changes
            response_id, response = self.__handle_all_task_request(msg)
            self.on_push(Task.serviceid, response_id, response)
        else:
            logging.warning(
                'Can\'t delete unknown task \'%s\'...' % msg['uid'])

        return Task.NULL, dict()

    def __reminder(self, uid):
        logging.debug('Push reminder for \'%s\'...' % uid)
        msg = {'uid': uid, 'reminder': self.tasks[uid].reminder}
        self.on_push(Task.serviceid, Task.REMINDER, msg)
