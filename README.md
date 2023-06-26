# alsd

`alsd` utilizes the [gpioals](https://github.com/axkg/gpioals) driver to measure ambient light intensivity with a light dependent resistor connected to a GPIO pin of a *Raspberry Pi* and distributes the measurements via MQTT.

This tool is based on and borrows some basic functionality from [buzzd](https://github.com/axkg/buzzd).

## Prerequisites

### GPIOALS driver

The [gpioals](https://github.com/axkg/gpioals) module must be compiled and loaded for the current kernel (so that the device `/dev/gpioals_device` is accessible) and the hardware wired to the correct GPIO pins.

In order for for `alsd` to be able to access the device, you will either have to run `alsd` as root user or ensure that the device is accessible through some customized udev rules.

## Configuration

On startup, `alsd` will try to read its configuration file `alsd.json`. It will try to find it in:

* the current directory
* the `.config` directory in the executing user's home
* in `/etc`

### JSON structure

Please refer to the [example configuration file](alsd.json) that comes with `alsd` to adapt to your use case. The following parameters can be set at top level:

* `device`: The path to the gpioals character device (`/dev/gpioals_device` by default)
* `rate`: The measurement rate in milliseconds, note that and additional 1.000 ms will be added for preparation of the measurement. The default value of 14.000 ms should yield a measurement roughly every 15 seconds. Increase the `rate` value if you want to reduce the amount of measurements collected. Be careful when decreasing below the default: In dark environments each measurement can require a multiple seconds to complete.

The connection to the MQTT broker can be setup in the `mqtt` section:

* `broker`: IP address or server name of the MQTT broker, `localhost` by default - according to the paho-mqtt documentation URIs should work, too (e.g. `mqtt://server:port`) - but that does not work for me currently
* `topic`: The MQTT topic `alsd` should send the measured values for, `alsd` by default

## Running alsd

Upon execution `alsd` will trigger periodic measurments and send the observed charging time as integer values with the configured MQTT topic. In bright environments one should receive low values. The darker the environment, the higher the measured values. If the light intensity falls below the detectable threshold, no measurements will be sent.
