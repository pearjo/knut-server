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
import json
import logging
import os
import signal
import time
import uuid


class Task(Events):
    """A Knut task service.

    This class represents a Knut task service. A task can have a :attr:`due`
    date and time with a :attr:`reminder` in seconds before the :attr:`due`.
    When the reminder is due, the event :meth:`on_remind()` is called. Objects
    registered to this event can react upon the notification and send a reminder
    to clients.
    """

    def __init__(self, uid=None, dir=None):
        self.uid = uid
        """The unique id of the task."""
        self.dir = dir
        """The directory where the task can be saved."""

        self.assignee = str()
        """The user to which the task is assigned."""
        self.author = str()
        """The name of the task author."""
        self.body = str()
        """The body text of the task."""
        self.done = False
        """Whether the task is done or not."""
        self.due = int()
        """The due date time of the task in seconds since the epoch January 1,
        1970, 00:00:00 (UTC)."""
        self.reminder = int()
        """The time in seconds before *due*. When the time is reached, a
        reminder notification is send."""
        self.title = str()
        """The task title."""

        if not self.uid:
            self.uid = uuid.uuid1()

        # Call on_remind as method with the uid as argument to notify
        # listening methods.
        self.__events__ = ('on_remind')

    def update_task(self, task):
        """Updates the task to the parsed *task* dictionary."""

        logging.debug('Update task \'%s\'...' % str(self.uid))

        if 'assignee' in task.keys():
            self.assignee = task['assignee']

        if 'author' in task.keys():
            self.author = task['author']

        if 'body' in task.keys():
            self.body = task['body']

        if 'done' in task.keys():
            self.done = task['done']

        if 'due' in task.keys():
            self.due = task['due']

        if 'reminder' in task.keys():
            self.reminder = task['reminder']
            self._set_reminder()

        if 'title' in task.keys():
            self.title = task['title']

        self._save_task()

    def task(self):
        """Returns the task as dictionary."""
        return {
            'assignee': self.assignee,
            'author': self.author,
            'body': self.body,
            'done': self.done,
            'due': self.due,
            'reminder': self.reminder,
            'title': self.title,
            'uid': self.uid
        }

    def _save_task(self):
        if not self.dir:
            logging.warning(
                'No save directory for task \'%s\' set.' % self.uid
            )
            return

        dir = os.path.expanduser(self.dir)
        task = os.path.join(dir, '%s.json' % self.uid)

        try:
            os.makedirs(dir)
        except FileExistsError:
            pass

        with open(task, 'w') as f:
            json.dump(self.task(), f)

    def _set_reminder(self):
        def reminder_alarm(signum, frame):
            self.on_remind(self.uid)

        signal.alarm(0)  # disable the alarm

        # the time in seconds until the reminder is due
        time_from_now = int(round(self.due - self.reminder - time.time(), 0))

        if time_from_now < 0 or self.done:
            return

        logging.debug('Set a reminder for \'%s\' which is due in %i seconds...'
                      % (str(self.uid), time_from_now))
        signal.signal(signal.SIGALRM, reminder_alarm)
        signal.alarm(time_from_now)
