// CLASSIFICATION: COMMUNITY
// Filename: REMOTE_ACCESS.md v1.0
// Date Modified: 2025-07-31
// Author: Lukas Bower

# REMOTE ACCESS

This document provides a bulletproof, step-by-step guide for a typical community member to connect a Cohesix Queen (in the cloud) to a Worker device on a home Wi‑Fi network behind a broadband router. It covers prerequisites, configuration steps, and common pitfalls.

## Assumptions & Prerequisites
1. **Worker Hardware**: Raspberry Pi, Jetson Nano, or similar edge device running Cohesix Worker image.
2. **Queen Service**: Cohesix Queen endpoint deployed in a public cloud with a static IP or DNS name.
3. **Network**: Home router providing NAT; internet uplink via ISP.  
4. **Credentials**: SSH key pair for secure access; admin privileges on router and devices.
5. **Software**:
   - `ssh`, `ssh-keygen`, `iptables` on Worker.
   - Dynamic DNS client (if no static IP).
   - Optional: VPN server (e.g., WireGuard) or reverse-tunnel service.

## Step 1: Configure Dynamic DNS (if needed)
1. If your ISP does not provide a static public IP, sign up for a Dynamic DNS provider (e.g., DuckDNS, No-IP).
2. Install and configure the DDNS client on the home network (router-level or on the Worker).
3. Verify that your domain (e.g., `myhome.duckdns.org`) resolves to your current public IP.

## Step 2: Set Up SSH Key Authentication
1. On your local machine, generate an SSH key pair (if you don’t have one):
   ```bash
   ssh-keygen -t ed25519 -C "cohesix-worker"
   ```
2. Copy the public key to the Worker:
   ```bash
   ssh-copy-id -i ~/.ssh/id_ed25519.pub user@worker.local
   ```
3. Confirm you can SSH into the Worker without a password:
   ```bash
   ssh user@worker.local
   ```

## Step 3: Configure Port Forwarding on the Router
1. Log in to your home router’s admin interface.
2. Allocate a static LAN IP for the Worker (e.g., `192.168.1.100`).
3. Forward an external port (e.g., TCP 2222) to the Worker’s SSH port (22):
   - Source port: 2222 → Destination IP: `192.168.1.100`, port 22.
4. Save and apply the configuration.
5. Test connectivity from outside your network:
   ```bash
   ssh -p 2222 user@<Your_Public_IP_or_DDNS>
   ```

## Step 4: Harden SSH & Firewall on the Worker
1. Edit `/etc/ssh/sshd_config`:
   - Disable password authentication: `PasswordAuthentication no`
   - Change SSH port to match forwarded port if desired.
   - Restrict root login: `PermitRootLogin no`
2. Restart SSH service:
   ```bash
   sudo systemctl restart sshd
   ```
3. Configure `iptables` or `ufw` to allow only the forwarded port and loopback:
   ```bash
   sudo ufw default deny incoming
   sudo ufw allow 2222/tcp  # or the port forwarded from the router
   sudo ufw enable
   ```
4. Verify firewall rules:
   ```bash
   sudo ufw status verbose
   ```

## Step 5: Establish a Secure 9P/TLS or Reverse Tunnel (Optional Alternative)
- **Direct TLS**:
  1. Generate certificates (CA-signed or self-signed) on the Queen.
 2. Configure the Worker to mount the Cohesix 9P namespace over TLS using `coh-9p-mount`:
    ```bash
    coh-9p-mount --tls --host queen.example.com --port 564 --mountpoint /srv/coh
    ```
- **Reverse SSH Tunnel** (if forward ports blocked or ISP CGNAT):
  1. On the Worker, create a persistent reverse tunnel:
     ```bash
     autossh -M 0 -f -N -R 2222:localhost:22 user@queen.example.com
     ```
  2. On the Queen, connect back to the Worker:
     ```bash
     ssh -p 2222 user@localhost
     ```

## Step 6: Validate Connection & Cohesix Services
1. From the Queen, test communication:
   ```bash
   coh-svc ping --worker my-worker-id
   ```
2. Check logs on both Worker and Queen for errors:
   ```bash
   sudo journalctl -u coh-worker
   sudo journalctl -u coh-queen
   ```
3. Run a sample workload to confirm end-to-end functionality.
4. (Optional) Validate worker registration and tracing:
   ```bash
   cohtrace list --scope worker
   ```

## Common Pitfalls & Troubleshooting
- **Double NAT / CGNAT**: ISP uses carrier-grade NAT—port forwarding won’t work. Use reverse SSH tunnel or VPN.
- **Dynamic IP Delays**: DDNS updates lag behind IP changes. Use router-based client if possible.
- **Firewall Conflicts**: Ensure both router and Worker firewall allow the chosen ports.
- **SSH Key Mismatch**: Verify the correct public key is in `/home/user/.ssh/authorized_keys`.
- **Service Startup**: Enable Cohesix services to start on boot:
  ```bash
  sudo systemctl enable coh-worker
  ```
- **DNS Propagation**: New DDNS entries may take minutes to resolve globally.

## Additional Private Documentation
For parallel business development, consider classifying these as PRIVATE:
- VPN server configuration and credentials
- SSH key management policies and rotation schedule
- Detailed network topology diagrams with IP ranges
- Automated deployment scripts with embedded secrets
