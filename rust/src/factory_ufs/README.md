# ChromeOS Factory UFS

Factory UFS is used to do UFS provisioning.

## Build
Factory UFS is built with `factory_ufs`:

```
(inside chroot)
cros-workon --board $BOARD start factory_ufs
emerge-$BOARD factory_ufs
```

then you can find the built binary in `/build/$BOARD/usr/sbin/factory_ufs`.