from flask import Flask, request
from flask_socketio import SocketIO
from knut.core import KnutConfig
import json
import knut.apis


class KnutApplication():
    def __init__(self, name):
        self.app = Flask(name)

        self.app.add_url_rule('/knut',
                              view_func=self.knut,
                              methods=['GET', 'POST'])

        self.apis = dict()
        self.socketio = SocketIO(self.app)

    def knut(self):
        try:
            msg = json.loads(request.args.get('msg'))
        except (json.decoder.JSONDecodeError, TypeError):
            msg = dict()

        msgid = int(request.args.get('msgId'))
        apiid = int(request.args.get('apiId'))

        if apiid in self.apis:
            msgid, msg = self.apis[apiid].request_handler(
                msgid, msg)

        return {'msg': msg, 'msgId': msgid, 'apiId': apiid}

    def run(self):
        self.socketio.run(self.app)


knutapp = KnutApplication(__name__)

# load config
config = KnutConfig('../../etc/example.yml').config

# load task module
task = config['task']
task.load_tasks()
knutapp.apis[task.apiid] = task

# load temperature module
try:
    temp = knut.apis.Temperature()
    for backend in config['temperature']:
        temp.add_backend(backend)

    knutapp.apis[temp.apiid] = temp
except KeyError:
    pass

# load light module
try:
    light = knut.apis.Light()
    for backend in config['lights']:
        light.add_backend(backend)

    knutapp.apis[light.apiid] = light
except KeyError:
    pass

# load local module
if config['local']:
    local = knut.apis.Local()
    local.set_local(config['local'])
    knutapp.apis[local.apiid] = local

app = knutapp.app

if __name__ == '__main__':
    knutapp.run()
