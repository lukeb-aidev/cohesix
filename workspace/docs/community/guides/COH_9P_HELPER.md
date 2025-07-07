// CLASSIFICATION: COMMUNITY
// Filename: COH_9P_HELPER.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-09-01

# coh-9p-helper

`coh-9p-helper` proxies 9P traffic between a TCP port and a Unix socket. It is
used during development to bridge Secure9P mounts.

## Flags

| Flag | Description | Default |
|------|-------------|---------|
| `--listen` | TCP address to listen on | `:5640` |
| `--socket` | Unix socket path for 9P server | `/srv/coh9p.sock` |

## Example

```rc
coh-9p-helper --listen :5640 --socket /srv/coh9p.sock &
```

The service logs connection errors to standard error. Use with
`secure9p` to forward requests to remote nodes.
