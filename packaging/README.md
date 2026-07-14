# BaeSQL Packaging

This directory contains Debian arm64 packaging metadata and Raspberry Pi example configuration for BaeSQL.

The GitHub release workflow builds the installable `.deb` artifact on an arm64 runner and installs the binary at:

```text
/usr/bin/baesql
```

The package does not create or remove `.bae` database files.
