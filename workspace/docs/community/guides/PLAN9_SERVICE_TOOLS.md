// CLASSIFICATION: COMMUNITY
// Filename: PLAN9_SERVICE_TOOLS.md v0.2
// Author: Lukas Bower
// Date Modified: 2027-09-01

# Plan9 Service Helper Tools

This guide covers usage of the `srvctl`, `indexserver`, and `devwatcher` utilities included in Cohesix.

## srvctl
Register services under `/srv/services`.
| Flag | Description | Default |
|------|-------------|---------|
| `announce -name` | service name | none |
| `announce -version` | service version | `0` |
```rc
srvctl announce -name demo -version 0.1 /mnt/test
```

## indexserver
Searchable file index exposed via `/srv/index`.

| Flag | Description | Default |
|------|-------------|---------|
| `--root` | root directory to index | `/` |
| `--interval` | query poll interval | `1s` |

```rc
indexserver &
echo "gpu" > /srv/index/query
cat /srv/index/results
```

## devwatcher
File change watcher with output in `/dev/watch`.

| Flag | Description | Default |
|------|-------------|---------|
| `--interval` | poll interval | `1s` |

```rc
devwatcher &
echo /tmp/foo.txt > /dev/watch/ctl
cat /dev/watch/events
```

