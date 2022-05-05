# ChromeOS Factory FAI

A “first article inspection” flow for ChromeOS devices in the factories. To make
sure critical data is provisioned to the device.

## Build
Factory FAI is built with `factory_installer`:

```
(inside chroot)
cros_workon --board $BOARD start factory_installer
emerge-$BOARD factory_installer
```

then you can find the built binary in `/build/$BOARD/usr/sbin/factory_fai`.

## Usage
To be updated.
```
(on DUT)
/usr/sbin/factory_fai
```
