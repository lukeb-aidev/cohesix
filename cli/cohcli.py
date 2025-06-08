
#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: cohcli.py v0.8
# Date Modified: 2025-07-10
# Author: Lukas Bower

"""
CohCLI – Command-line interface for interacting with Cohesix services.
"""

import argparse
import sys
import os

def parse_args():
    parser = argparse.ArgumentParser(
        description="CohCLI – Manage and interact with Cohesix nodes and services."
    )

    subparsers = parser.add_subparsers(dest="command", help="CohCLI subcommands")

    # Subcommand: status
    parser_status = subparsers.add_parser("status", help="Check node and service status")
    parser_status.add_argument("--verbose", action="store_true", help="Show detailed output")

    # Subcommand: boot
    parser_boot = subparsers.add_parser("boot", help="Trigger boot or reboot sequence")
    parser_boot.add_argument("role", help="Role to boot (e.g., QueenPrimary, DroneWorker)")

    # Subcommand: trace
    parser_trace = subparsers.add_parser("trace", help="View recent trace logs")
    parser_trace.add_argument("--filter", help="Filter by subsystem or agent name")

    # Subcommand: trace-violations
    subparsers.add_parser("trace-violations", help="Show runtime violation log")

    # Subcommand: replay-trace
    replay = subparsers.add_parser("replay-trace", help="Replay a trace file")
    replay.add_argument("path")
    upg = subparsers.add_parser("upgrade", help="Upgrade system")
    upg.add_argument("--from", dest="src", required=True)
    subparsers.add_parser("rollback", help="Rollback last upgrade")
    subparsers.add_parser("list-models", help="List available models")
    dec = subparsers.add_parser("decrypt-model", help="Decrypt model")
    dec.add_argument("model")
    ver = subparsers.add_parser("verify-model", help="Verify model signature")
    ver.add_argument("model")
    ens = subparsers.add_parser("agent-ensemble-status", help="Show ensemble status")
    ens.add_argument("ensemble")

    # Subcommand: dispatch-slm
    disp = subparsers.add_parser("dispatch-slm", help="Dispatch SLM to worker")
    disp.add_argument("--target", required=True)
    disp.add_argument("--model", required=True)

    # Subcommand: agent lifecycle
    parser_agent = subparsers.add_parser("agent", help="Agent lifecycle commands")
    agent_sub = parser_agent.add_subparsers(dest="agent_cmd")

    start_cmd = agent_sub.add_parser("start", help="Start an agent")
    start_cmd.add_argument("agent_id")
    start_cmd.add_argument("--role", required=True)

    pause_cmd = agent_sub.add_parser("pause", help="Pause an agent")
    pause_cmd.add_argument("agent_id")

    mig_cmd = agent_sub.add_parser("migrate", help="Migrate an agent")
    mig_cmd.add_argument("agent_id")
    mig_cmd.add_argument("--to", required=True)

    # Subcommand: sim
    parser_sim = subparsers.add_parser("sim", help="Simulation utilities")
    sim_sub = parser_sim.add_subparsers(dest="sim_cmd")
    run_cmd = sim_sub.add_parser("run", help="Run a simulation")
    run_cmd.add_argument("scenario")

    # Subcommand: federation
    parser_fed = subparsers.add_parser("federation", help="Manage queen federation")
    fed_sub = parser_fed.add_subparsers(dest="fed_cmd")
    conn_cmd = fed_sub.add_parser("connect", help="Connect to a peer queen")
    conn_cmd.add_argument("peer")
    dis_cmd = fed_sub.add_parser("disconnect", help="Disconnect from a peer")
    dis_cmd.add_argument("peer")
    fed_sub.add_parser("list", help="List peers")
    fed_sub.add_parser("monitor", help="Show federation log")

    # Subcommand: run-inference
    inf = subparsers.add_parser("run-inference", help="Run webcam inference task")
    inf.add_argument("worker")
    inf.add_argument("task")

    # Subcommand: sync-world
    syncw = subparsers.add_parser("sync-world", help="Sync world model to worker")
    syncw.add_argument("--target", required=True)

    exportw = subparsers.add_parser("export-world", help="Export world model")
    exportw.add_argument("--path", required=True)

    policy_show = subparsers.add_parser("show-policy", help="Show agent policy memory")
    policy_show.add_argument("agent")

    policy_wipe = subparsers.add_parser("wipe-policy", help="Wipe agent policy memory")
    policy_wipe.add_argument("agent")

    vis = subparsers.add_parser("vision-overlay", help="Run vision overlay")
    vis.add_argument("--agent", required=True)
    vis.add_argument("--save", action="store_true")

    stream = subparsers.add_parser("stream-overlay", help="Stream overlay")
    stream.add_argument("--port", required=True)

    intros = subparsers.add_parser("agent-introspect", help="Show agent introspection log")
    intros.add_argument("agent_id")

    elect = subparsers.add_parser("elect-queen", help="Elect new queen from peers")
    elect.add_argument("--mesh", required=True)

    assume = subparsers.add_parser("assume-role", help="Assume cluster role")
    assume.add_argument("role")

    return parser.parse_args()

def main():
    args = parse_args()

    if args.command == "status":
        handle_status(args)
    elif args.command == "boot":
        handle_boot(args)
    elif args.command == "trace":
        handle_trace(args)
    elif args.command == "trace-violations":
        handle_trace_violations()
    elif args.command == "replay-trace":
        handle_replay(args)
    elif args.command == "dispatch-slm":
        handle_dispatch_slm(args)
    elif args.command == "agent":
        handle_agent(args)
    elif args.command == "sim":
        handle_sim(args)
    elif args.command == "run-inference":
        handle_inference(args)
    elif args.command == "federation":
        handle_federation(args)
    elif args.command == "sync-world":
        handle_sync_world(args)
    elif args.command == "export-world":
        handle_export_world(args)
    elif args.command == "show-policy":
        handle_show_policy(args)
    elif args.command == "wipe-policy":
        handle_wipe_policy(args)
    elif args.command == "vision-overlay":
        handle_vision_overlay(args)
    elif args.command == "stream-overlay":
        handle_stream_overlay(args)
    elif args.command == "agent-introspect":
        handle_agent_introspect(args)
    elif args.command == "elect-queen":
        handle_elect_queen(args)
    elif args.command == "assume-role":
        handle_assume_role(args)
    else:
        print("No command provided. Use -h for help.")
        sys.exit(1)

def handle_status(args):
    import os
    role = os.environ.get("COH_ROLE", "Unknown")
    print(f"Node status OK – role: {role}")
    if args.verbose:
        print("Environment variables:")
        for k, v in os.environ.items():
            if k.startswith("COH"):  # show Cohesix-related vars
                print(f"  {k}={v}")

def handle_boot(args):
    print(f"Booting role: {args.role} ...")
    try:
        from cohesix.runtime.env import init
        init.initialize_runtime_env()
        os.environ["COH_ROLE"] = args.role
        print("Boot sequence complete")
    except Exception as e:
        print(f"Boot failed: {e}")

def handle_trace(args):
    filter_val = args.filter or "*"
    print(f"Showing trace log entries matching '{filter_val}' (stub)")

def handle_trace_violations():
    path = "/srv/violations/runtime.json"
    if os.path.exists(path):
        print(open(path).read())
    else:
        print("No violations logged")

def handle_replay(args):
    from pathlib import Path
    from scripts import cohtrace
    trace = cohtrace.read_trace(Path(args.path))
    for ev in trace:
        print(f"replay {ev['event']} {ev['detail']}")

def handle_dispatch_slm(args):
    req_dir = f"/srv/slm/dispatch/{args.target}"
    os.makedirs(req_dir, exist_ok=True)
    req_path = os.path.join(req_dir, f"{args.model}.req")
    open(req_path, "w").write("1")
    print(f"Dispatch request for {args.model} sent to {args.target}")

def handle_agent(args):
    if args.agent_cmd == "start":
        print(f"Starting agent {args.agent_id} with role {args.role}")
    elif args.agent_cmd == "pause":
        print(f"Pausing agent {args.agent_id}")
    elif args.agent_cmd == "migrate":
        print(f"Migrating agent {args.agent_id} to {args.to}")
        try:
            from cohesix import agent_migration
            agent_migration.migrate(args.agent_id, args.to)
        except Exception as e:
            print(f"Migration failed: {e}")
    else:
        print("Unknown agent command")

def handle_inference(args):
    import subprocess
    env = os.environ.copy()
    env["INFER_CONF"] = args.task
    script = os.path.join(os.path.dirname(__file__), "../scripts/worker_inference.py")
    subprocess.run(["python3", script], env=env, check=False)

def handle_federation(args):
    if args.fed_cmd == "connect":
        path = f"/srv/federation/requests/{args.peer}.connect"
        os.makedirs("/srv/federation/requests", exist_ok=True)
        open(path, "w").write("1")
        print(f"Connect request sent to {args.peer}")
    elif args.fed_cmd == "disconnect":
        path = f"/srv/federation/requests/{args.peer}.disconnect"
        os.makedirs("/srv/federation/requests", exist_ok=True)
        open(path, "w").write("1")
        print(f"Disconnect request sent to {args.peer}")
    elif args.fed_cmd == "list":
        for f in os.listdir("/srv/federation/known_hosts"):
            print(f)
    elif args.fed_cmd == "monitor":
        log = "/srv/federation/events.log"
        if os.path.exists(log):
            print(open(log).read())
    else:
        print("Unknown federation command")

def handle_sim(args):
    if args.sim_cmd == "run" and args.scenario == "BalanceBot":
        try:
            from cohesix.sim.physics_adapter import PhysicsAdapter
            adapter = PhysicsAdapter.new()
            adapter.run_balance_bot(100)
            print("BalanceBot simulation complete")
        except Exception as e:
            print(f"Simulation failed: {e}")
    else:
        print("Unknown simulation scenario")

def handle_sync_world(args):
    path = f"/srv/world_sync/{args.target}.json"
    if os.path.exists(path):
        print(f"Synced world model to {args.target}")
    else:
        print(f"No snapshot for {args.target}")

def handle_export_world(args):
    src = "/srv/world_model/world.json"
    if os.path.exists(src):
        import shutil
        shutil.copy(src, args.path)
        print(f"World model exported to {args.path}")
    else:
        print("World model not found")

def handle_show_policy(args):
    path = f"/persist/policy/agent_{args.agent}.policy.json"
    if os.path.exists(path):
        print(open(path).read())
    else:
        print("No policy found")

def handle_wipe_policy(args):
    path = f"/persist/policy/agent_{args.agent}.policy.json"
    if os.path.exists(path):
        os.remove(path)
        print("Policy wiped")
    else:
        print("No policy found")

def handle_vision_overlay(args):
    print(f"Running vision overlay for {args.agent} (save={args.save})")

def handle_stream_overlay(args):
    print(f"Streaming overlay on port {args.port}")

def handle_agent_introspect(args):
    path = f"/trace/introspect_{args.agent_id}.log"
    if os.path.exists(path):
        print(open(path).read())
    else:
        print("No introspection data")

def handle_elect_queen(args):
    print(f"Electing queen using {args.mesh}")

def handle_assume_role(args):
    open("/srv/queen/role", "w").write(args.role)

def handle_upgrade(args):
    os.makedirs("/srv/upgrade", exist_ok=True)
    open("/srv/upgrade/url", "w").write(args.src)
    print(f"Upgrade request for {args.src}")

def handle_rollback(args):
    os.makedirs("/srv/upgrade", exist_ok=True)
    open("/srv/upgrade/rollback", "w").write("1")
    print("Rollback requested")

def handle_list_models(args):
    base = "/persist/models"
    if os.path.isdir(base):
        for f in os.listdir(base):
            if f.endswith(".slmcoh"):
                print(f)
    else:
        print("No models")

def handle_decrypt_model(args):
    from cryptography.hazmat.primitives.ciphers.aead import AESGCM
    path = os.path.join("/persist/models", args.model)
    data = open(path, "rb").read()
    nonce, ct = data[:12], data[12:]
    key = b"0" * 32
    plain = AESGCM(key).decrypt(nonce, ct, None)
    os.makedirs("/srv/models", exist_ok=True)
    out = os.path.join("/srv/models", args.model + ".bin")
    open(out, "wb").write(plain)
    print(f"Decrypted to {out}")

def handle_verify_model(args):
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    path = os.path.join("/persist/models", args.model)
    sig_path = path + ".sig"
    key_path = "/keys/slm_signing.pub"
    if not os.path.exists(sig_path) or not os.path.exists(key_path):
        print("Missing signature or key")
        return
    pub = Ed25519PublicKey.from_public_bytes(open(key_path, "rb").read())
    data = open(path, "rb").read()
    sig = open(sig_path, "rb").read()
    try:
        pub.verify(sig, data)
        print("Signature OK")
    except Exception:
        print("Invalid signature")

def handle_agent_ensemble_status(args):
    path = f"/ensemble/{args.ensemble}/goals.json"
    if os.path.exists(path):
        print(open(path).read())
    else:
        print("No ensemble data")
if __name__ == "__main__":
    main()
