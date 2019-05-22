Master branch: [![Build Status](https://travis-ci.org/tolien/currentcost-rs.svg?branch=master)](https://travis-ci.org/tolien/currentcost-rs)

# currentcost-rs

A client for listening to a Currentcost device through a serial port.

A config file is required, for example:
```[database]
db_name = "currentcost"
hostname = "/var/run/postgresql"
user = "db_user"

[serial]
port = "/dev/ttyUSB1"
bit_rate = 57600
timeout = 5

``` 
this should be called config.toml and is expected to be in the same place as the compiled binary.
