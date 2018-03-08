# Chrome OS Factory Shim

## Introduction
This folder contains the major scripts for the "Chrome OS factory shim".
The shim is used for installing a Chrome OS image (kernel, rootfs and firmware)
to a device. It's also known as "(factory) install shim", "RMA shim", or "Reset
shim".

The factory shim is designed to allow operators removing USB stick once it's
booted, so the boot process is slightly different. The shim relies on
`initramfs` technology to bootstrap and load all contents into memory, then
start an upstart service to display the menu.

## Building a factory shim
Inside chroot, do:

    build_packages --board $BOARD
    build_image --board $BOARD factory_install

The output disk image is in
`~/trunk/src/build/images/$BOARD/latest/factory_install_shim.bin`.

If you have local changes in `src/platform/factory_installer`, please remember
to do

    cros_workon --board $BOARD start factory_installer
    emerge-$BOARD factory_installer

If you have local changes in `src/platform/initramfs`, please remember to do

    cros_workon --board $BOARD start chromeos-initramfs

There's no need to emerge `chromeos-initramfs` because it's always re-built in
`build_image` stage.

## Using factory shim
The factory shims were signed in a special way for security reasons. It needs to
boot with "developer switch turned on" and "boot in recovery mode".

If you have a signed factory shim, follow the steps to boot:
1. Enter recovery mode (`Esc-F3-Power`)
2. `Ctrl-D` to request turn on developer switch, and `ENTER` to confirm.
3. Enter recovery mode again and insert factory shim.

If you boot factory shim in developer mode (`Ctrl-U`), few functions won't work,
for example recovering TPM.

## Debugging a factory shim
Factory shims do not provide shells by default for security reason. If you can
still see virtual terminal consoles, try VT0, VT1, VT2, VT3 - there are lots of
debug messages there.

### Getting a shell
If you do need a shell to debug, add `cros_debug` to kernel command line. You
can do this in `build_image`:

    build_image --board $BOARD --boot_args cros_debug factory_install

For an existing image, you can use `make_dev_ssd.sh` to change kernel command
line easily:

    # inside chroot
    cd ~/trunk/src/platform/vboot_reference/scripts/image_signing
    ./make_dev_ssd.sh -i $PATH_TO_IMAGE_OR_USB_DEVICE \
       --partitions 2 --recovery --edit_config

This will bring an editor to allow editing command line.

Note `make_dev_ssd.sh` is also available on all Chrome OS image (even factory
shim) - try `/usr/share/vboot/bin/make_dev_ssd.sh`.

If you boot a factory shim with `cros_debug`, then you should have one shell in
VT2 or VT3. Moreover, if you can enter the menu, 'S' will give you the full
shell.

### Fail to start frecon
The `frecon` (or `frecon-lite`) provides text-based console. If you can't see
anything on screen, redirect the console to another device, for example Servo
consoles so you can check why `frecon` failed. To do this, add
`console=ttyS0,115200n8` to kernel command line (use the `make_dev_ssd.sh` or
add `--boot_args` as explained in previous section). Some devices may need
different TTY name for example `ttyS1`. Please check the care-and-feed doc of
your device.
