// CLASSIFICATION: COMMUNITY
// Filename: PLAN9_SERVICE_TOOLS.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-02-01

# Plan9 Service Helper Tools

This guide covers usage of the `srvctl`, `indexserver`, and `devwatcher` utilities included in Cohesix.

## srvctl
Register services under `/srv/services`.
```rc
srvctl announce -name demo -version 0.1 /mnt/test
```

## indexserver
Searchable file index exposed via `/srv/index`.
```rc
echo "gpu" > /srv/index/query
cat /srv/index/results
```

## devwatcher
File change watcher with output in `/dev/watch`.
```rc
echo /tmp/foo.txt > /dev/watch/ctl
cat /dev/watch/events
```

