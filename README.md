<p align="center">
  <img src="knut.png" title="Knut"><br>
</p>

# Knut: your humble server!
[![Build Status](https://travis-ci.org/pearjo/knut-server.svg?branch=devel)](https://travis-ci.org/pearjo/knut-server)
[![Documentation Status](https://readthedocs.org/projects/knut-server/badge/?version=latest)](https://knut-server.readthedocs.io/en/latest/?badge=latest)
[![CodeFactor](https://www.codefactor.io/repository/github/pearjo/knut-server/badge)](https://www.codefactor.io/repository/github/pearjo/knut-server)
[![License](https://img.shields.io/badge/License-GPL%20v3-blue.svg)](https://github.com/pearjo/knut-server/blob/master/LICENSE)

*Knut* is a friendly penguin to help organize your home.

Ok... What is Knut again? It's a smart home assistant with a server at
it's core which is connected to various
[APIs](https://knut-server.readthedocs.io/en/latest/apis.html). Via a
JSON formatted message, clients can interact with the various
APIs. They are designed in such way, that they can be extended
modular. Each API is then connected with
[services](https://knut-server.readthedocs.io/en/latest/reference/services.html)
which do some work like switching
e.g. [TRÅDFRI](https://www.ikea.com/de/de/product-guides/tradfri-home-smart-beleuchtung-pub61503271)
lights or providing [OpenWeather](https://openweathermap.org/) data to
the API.

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

The Knut server is looking for the
[configuration](https://knut-server.readthedocs.io/en/latest/config.html)
file `/etc/knutconfig.yaml` but will use a fail-safe configuration if
the file is not found. The server is started by running the following:

```bash
knutserver.py
```

To run in debug mode, use `--log=DEBUG` as additional argument.

You can also take Knut for a test run using the example configuration
`etc/example.yml`:

```bash
knutserver.py --conf=etc/example.yml --log=DEBUG
```

For more, please [read the docs](https://knut-server.readthedocs.io)
to see which APIs and services are available and how they are
configuration.
