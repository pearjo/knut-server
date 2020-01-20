from knutservices import Temperature
import logging
import pickle
import requests
import threading
import time

OWM_TO_WEATHER_ICON = {
    '01d': 'day-sunny',
    '02d': 'day-cloudy',
    '03d': 'cloud',
    '04d': 'cloudy',
    '09d': 'day-showers',
    '10d': 'day-rain',
    '11d': 'day-thunderstorm',
    '13d': 'day-snow',
    '50d': 'day-fog',
    '01n': 'night-clear',
    '02n': 'night-cloudy',
    '03n': 'cloud',
    '04n': 'cloudy',
    '09n': 'night-showers',
    '10n': 'night-rain',
    '11n': 'night-thunderstorm',
    '13n': 'night-snow',
    '50n': 'night-fog',
}


class OpenWeatherMap(Temperature):
    def __init__(self, location, unique_name, **kwargs):
        super(OpenWeatherMap, self).__init__(location, unique_name)
        self.data = dict()  # stores data received from OpenWeatherMap
        self.url = str('http://api.openweathermap.org/data/2.5/weather?'
                       + 'q=' + location
                       + '&APPID=' + kwargs['appid'])

        # start the data poller daemon
        daemonThread = threading.Thread(target=self.daemon,
                                        name='daemonThread')
        daemonThread.daemon = True
        daemonThread.start()

    def request_data(self):
        try:
            self.data = requests.get(self.url).json()
            self.location = self.data['name']
            if self.data:
                self.temperature = self.data['main']['temp'] - 273.15
                self.condition = OWM_TO_WEATHER_ICON[self.data['weather'][0]['icon']]
        except:
            logging.warning('Can not connect to api.openweathermap.org.')

    def daemon(self):
        while True:
            previus_data = self.data
            self.request_data()

            # push data only if changed
            if previus_data != self.data:
                self.on_change(self.unique_name)

            # clear history after day changed
            try:
                if (time.strptime(self.history[1][-1], "%H:%M:%S")
                        > time.localtime()):
                    self.history = [list(), list()]
            except IndexError:
                pass

            self.history[0].append(self.temperature)
            self.history[1].append(time.strftime("%H:%M:%S"))

            with open(self.data_file, 'wb') as f:
                # store the data as binary data stream
                logging.debug('Dump temperature history of \'%s\' to file.' %
                              self.unique_name)
                pickle.dump(self.history, f)

            time.sleep(90)  # wait 1.5 minutes to not exceed the polling limit
