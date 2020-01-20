# Knut Server

![Image of Knut](knut.png)

*Knut* is a friendly penguin to help organize your home.

Knut has various APIs to control e.g. lights or supply temperature
data from various sources. The API communicates with *services* which
are implementing the actions needed to e.g. switch a
[TRÅDFRI](https://www.ikea.com/de/de/product-guides/tradfri-home-smart-beleuchtung-pub61503271)
light.

## Installation

Knut uses Python 3.7 and the required packages can be install by
running the following:

```bash
pip install -r requirements.txt
```

To install the Knut package run:

```bash
python setup.py install
```

The code documentation can be build by running the following:

```bash
cd doc
make html
```

## Usage

The Knut server is looking for a configuration in
`/etc/knutconfig.yaml` which has the following structure:

```yaml
---
# TCP socket configuration
socket:
  ip: 127.0.0.1
  port: 8080

---
# API configuration
#
# Each API can have multiple services. Each service has a name which
# must be unique, a serviceid as hex, a location to which the service relates,
# the module which provides the service and the object which is the service
# class. Additional keyword arguments which are needed for an object are parsed
# by the kwargs key.
temperature:
  localWeather: # localWeather is the unique name
    serviceid: 0x01
    location: Hamburg
    module: knutservices.dummytemperature
    object: DummyTemperature

light:
  advancedLight:
    serviceid: 0x02
    location: Sofa Light
    module: knutservices.dummylight
    object: DummyLight
    # the object DummyLight requires multiple keyword arguments
    kwargs:
      room: Living Room
      dimlevel: true
      color: true
      temperature: true
      colorCold: "#f5faf6"
      colorWarm: "#efd275"
```

Please read the docs to see which APIs and services are available and
what keyword arguments they need for their configuration. With a
proper configuration in place, the Knut server is run by:

```bash
knut.py
```

To run in debug mode, use `--log=DEBUG` as additional argument.
