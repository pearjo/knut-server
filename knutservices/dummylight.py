from knutservices import Light
import logging


class DummyLight(Light):
    """A dummy light."""
    def __init__(self, location, unique_name, room, **kwargs):
        super(DummyLight, self).__init__(location, unique_name, room)
        self.has_dimlevel = kwargs['dimlevel']
        self.has_color = kwargs['color']
        self.has_temperature = kwargs['temperature']

        self.temperature = 80
        self.dimlevel = 0
        self.state = False
        self.color = '#ff1122'

        # set color temperature limits if provided
        if self.has_temperature:
            try:
                self.color_cold = kwargs['color_warm']
                self.color_warm = kwargs['color_cold']
            except KeyError:
                # TODO: set better fallback values
                self.color_cold = '#ffffff'
                self.color_warm = '#ffffff'
                logging.warning('Unknown color temperature rang for'
                                '\'%s\'' % self.unique_name)
