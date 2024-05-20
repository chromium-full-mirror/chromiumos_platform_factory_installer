# ChromeOS Factory FAI

A “first article inspection” flow for ChromeOS devices in the factories. To make
sure critical data is provisioned to the device.

## User Manual

Factory FAI is built-in with Factory Install Shim.

After boot into factory shim, **do not** remove the USB install shim. You will
see the action item on the menu:
```
F Perform factory FAI  Perform Factory FAI (First Article Inspection)
```

then press F, the collected data wll be prompted on the console and the log
message will be showed up if FAI process success:
```
Succeed to save factory_fai_${MODEL}_${SN}.json in /dev/sda1."
Please upload the FAI data to issue tracker for Google review.
```

by default, FAI data will be saved in the stateful partition of the factory shim
with the filename `factory_fai_${MODEL}_${SN}.json`.

Assume that the USB drive is /dev/sdc, use the following commands to get the
reports:
```
mkdir /tmp/mount_point
mount /dev/sdc1 /tmp/mount_point
cp /tmp/mount_point/factory_fai_${MODEL}_*.json ./
```

## Development Guide

### Build
Factory FAI is built with `factory_installer_rust`:

```
(inside chroot)
cros_workon --board $BOARD start factory_installer_rust
emerge-$BOARD factory_installer_rust
```

The built binary can be found in `/build/$BOARD/usr/sbin/factory_installer`.

### Execute FAI from shell

Dump FAI data to `fai_data.json`:
```
(on DUT) $ /usr/sbin/factory_installer fai -o fai_data.json
```

Customize collected data by modifying config file:
```
(on DUT) $ /usr/sbin/factory_installer fai --dump-config /tmp/fai_config.json
# Modify your config
(on DUT) $ /usr/sbin/factory_installer fai -c /tmp/fai_config.json
```

### Configuration

You can customize the collected data by the configuration file. There are three
configuration classes for each FAI data:

- `DataCommand`:

Execute a command from command line and parse the data by parsers defined in
[parsers.rs](parsers.rs).

Example:
```
"DataCommand": {
  "cmd": "tpm_version",
  "parser": {
    "type": "DictParser",
    "delimiter": ":"
  }
}
```

- `DataFiles`:

List or read files under a directory.

Example:
```
"DataFiles": {
  "root": "/sys/firmware/vpd/ro",
  "read_content": true
}
```

- `BuiltInCollector`:

Use the functions defined in [built\_in\_collectors.rs](built_in_collectors.rs)
to collect data.

Example:
```
"BuiltInCollector": "signing_keys"
```

See more details and examples in `config.rs`.
