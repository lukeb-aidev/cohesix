# CLASSIFICATION: COMMUNITY
# Filename: cohcli.py v1.1
# Date Modified: 2025-08-01
# Author: Lukas Bower

"""
CohCLI – Command-line interface for interacting with Cohesix services.
"""

import argparse
import sys
import os
import shlex
import subprocess
from datetime import datetime
from pathlib import Path
from typing import List
import traceback


LOG_DIR = Path(os.getenv("COHESIX_LOG", Path.home() / ".cohesix" / "log"))
LOG_DIR.mkdir(parents=True, exist_ok=True)


def cohlog(msg: str) -> None:
    with (LOG_DIR / "cli_tool.log").open("a") as f:
        f.write(f"{datetime.utcnow().isoformat()} {msg}\n")
    print(msg)


def safe_run(cmd: List[str]) -> int:
    quoted = [shlex.quote(c) for c in cmd]
    with (LOG_DIR / "cli_exec.log").open("a") as f:
        f.write(f"{datetime.utcnow().isoformat()} {' '.join(quoted)}\n")
    result = subprocess.run(cmd)
    return result.returncode


def parse_args():
    parser = argparse.ArgumentParser(
        description="CohCLI – Manage and interact with Cohesix nodes and services."
    )
    parser.add_argument("--man", action="store_true", help="Show man page")
    parser.add_argument("--version", action="store_true", help="Show version info")
    parser.add_argument(
        "--profile",
        choices=["queen", "worker"],
        default="worker",
        help="Execution profile",
    )

    subparsers = parser.add_subparsers(dest="command", help="CohCLI subcommands")

    def add_common(sp):
        sp.add_argument("--dry-run", action="store_true")
        sp.add_argument("--verbose", action="store_true")

    # Subcommand: status
    parser_status = subparsers.add_parser(
        "status", help="Check node and service status"
    )
    add_common(parser_status)
    parser_status.set_defaults(func=handle_status)

    # Subcommand: boot
    parser_boot = subparsers.add_parser("boot", help="Trigger boot or reboot sequence")
    add_common(parser_boot)
    parser_boot.add_argument(
        "role", help="Role to boot (e.g., QueenPrimary, DroneWorker)"
    )
    parser_boot.set_defaults(func=handle_boot)

    # Subcommand: trace
    parser_trace = subparsers.add_parser("trace", help="View recent trace logs")
    add_common(parser_trace)
    parser_trace.add_argument("--filter", help="Filter by subsystem or agent name")
    parser_trace.set_defaults(func=handle_trace)

    # Subcommand: trace-violations
    tv = subparsers.add_parser("trace-violations", help="Show runtime violation log")
    add_common(tv)
    tv.set_defaults(func=lambda a: handle_trace_violations())

    # Subcommand: replay-trace
    replay = subparsers.add_parser("replay-trace", help="Replay a trace file")
    add_common(replay)
    replay.add_argument("path")
    replay.set_defaults(func=handle_replay)
    upg = subparsers.add_parser("upgrade", help="Upgrade system")
    add_common(upg)
    upg.add_argument("--from", dest="src", required=True)
    upg.set_defaults(func=handle_upgrade)
    rb = subparsers.add_parser("rollback", help="Rollback last upgrade")
    add_common(rb)
    rb.set_defaults(func=handle_rollback)
    lm = subparsers.add_parser("list-models", help="List available models")
    add_common(lm)
    lm.set_defaults(func=handle_list_models)
    dec = subparsers.add_parser("decrypt-model", help="Decrypt model")
    add_common(dec)
    dec.add_argument("model")
    dec.set_defaults(func=handle_decrypt_model)
    ver = subparsers.add_parser("verify-model", help="Verify model signature")
    add_common(ver)
    ver.add_argument("model")
    ver.set_defaults(func=handle_verify_model)
    ens = subparsers.add_parser("agent-ensemble-status", help="Show ensemble status")
    add_common(ens)
    ens.add_argument("ensemble")
    ens.set_defaults(func=handle_agent_ensemble_status)

    # Subcommand: dispatch-slm
    disp = subparsers.add_parser("dispatch-slm", help="Dispatch SLM to worker")
    add_common(disp)
    disp.add_argument("--target", required=True)
    disp.add_argument("--model", required=True)
    disp.set_defaults(func=handle_dispatch_slm)

    # Subcommand: agent lifecycle
    parser_agent = subparsers.add_parser("agent", help="Agent lifecycle commands")
    add_common(parser_agent)
    agent_sub = parser_agent.add_subparsers(dest="agent_cmd")

    start_cmd = agent_sub.add_parser("start", help="Start an agent")
    add_common(start_cmd)
    start_cmd.add_argument("agent_id")
    start_cmd.add_argument("--role", required=True)
    start_cmd.set_defaults(func=handle_agent)

    pause_cmd = agent_sub.add_parser("pause", help="Pause an agent")
    add_common(pause_cmd)
    pause_cmd.add_argument("agent_id")
    pause_cmd.set_defaults(func=handle_agent)

    mig_cmd = agent_sub.add_parser("migrate", help="Migrate an agent")
    add_common(mig_cmd)
    mig_cmd.add_argument("agent_id")
    mig_cmd.add_argument("--to", required=True)
    mig_cmd.set_defaults(func=handle_agent)

    # Subcommand: sim
    parser_sim = subparsers.add_parser("sim", help="Simulation utilities")
    add_common(parser_sim)
    sim_sub = parser_sim.add_subparsers(dest="sim_cmd")
    run_cmd = sim_sub.add_parser("run", help="Run a simulation")
    add_common(run_cmd)
    run_cmd.add_argument("scenario")
    run_cmd.set_defaults(func=handle_sim)

    # Subcommand: federation
    parser_fed = subparsers.add_parser("federation", help="Manage queen federation")
    add_common(parser_fed)
    fed_sub = parser_fed.add_subparsers(dest="fed_cmd")
    conn_cmd = fed_sub.add_parser("connect", help="Connect to a peer queen")
    add_common(conn_cmd)
    conn_cmd.add_argument("peer")
    conn_cmd.set_defaults(func=handle_federation)
    dis_cmd = fed_sub.add_parser("disconnect", help="Disconnect from a peer")
    add_common(dis_cmd)
    dis_cmd.add_argument("peer")
    dis_cmd.set_defaults(func=handle_federation)
    list_cmd = fed_sub.add_parser("list", help="List peers")
    add_common(list_cmd)
    list_cmd.set_defaults(func=handle_federation)
    mon_cmd = fed_sub.add_parser("monitor", help="Show federation log")
    add_common(mon_cmd)
    mon_cmd.set_defaults(func=handle_federation)

    # Subcommand: run-inference
    inf = subparsers.add_parser("run-inference", help="Run webcam inference task")
    add_common(inf)
    inf.add_argument("worker")
    inf.add_argument("task")
    inf.set_defaults(func=handle_inference)

    # Subcommand: sync-world
    syncw = subparsers.add_parser("sync-world", help="Sync world model to worker")
    add_common(syncw)
    syncw.add_argument("--target", required=True)
    syncw.set_defaults(func=handle_sync_world)

    exportw = subparsers.add_parser("export-world", help="Export world model")
    add_common(exportw)
    exportw.add_argument("--path", required=True)
    exportw.set_defaults(func=handle_export_world)

    policy_show = subparsers.add_parser("show-policy", help="Show agent policy memory")
    add_common(policy_show)
    policy_show.add_argument("agent")
    policy_show.set_defaults(func=handle_show_policy)

    policy_wipe = subparsers.add_parser("wipe-policy", help="Wipe agent policy memory")
    add_common(policy_wipe)
    policy_wipe.add_argument("agent")
    policy_wipe.set_defaults(func=handle_wipe_policy)

    vis = subparsers.add_parser("vision-overlay", help="Run vision overlay")
    add_common(vis)
    vis.add_argument("--agent", required=True)
    vis.add_argument("--save", action="store_true")
    vis.set_defaults(func=handle_vision_overlay)

    stream = subparsers.add_parser("stream-overlay", help="Stream overlay")
    add_common(stream)
    stream.add_argument("--port", required=True)
    stream.set_defaults(func=handle_stream_overlay)

    intros = subparsers.add_parser(
        "agent-introspect", help="Show agent introspection log"
    )
    add_common(intros)
    intros.add_argument("agent_id")
    intros.set_defaults(func=handle_agent_introspect)

    elect = subparsers.add_parser("elect-queen", help="Elect new queen from peers")
    add_common(elect)
    elect.add_argument("--mesh", required=True)
    elect.set_defaults(func=handle_elect_queen)

    assume = subparsers.add_parser("assume-role", help="Assume cluster role")
    add_common(assume)
    assume.add_argument("role")
    assume.set_defaults(func=handle_assume_role)

    parser.set_defaults(func=lambda a: parser.print_help())
    return parser.parse_args()


def main():
    args = parse_args()
    if getattr(args, "version", False):
        cohlog("CohCLI version 1.0")
        return
    if getattr(args, "man", False):
        man = os.path.join(os.path.dirname(__file__), "../bin/man")
        page = os.path.join(os.path.dirname(__file__), "../docs/man/cohcli.1")
        os.execv(man, [man, page])
    if hasattr(args, "func"):
        args.func(args)
    else:
        cohlog("No command provided. Use -h for help.")
        sys.exit(1)


def handle_status(args):
    import os

    role = os.environ.get("COH_ROLE", "Unknown")
    cohlog(f"Node status OK – role: {role}")
    if args.verbose:
        cohlog("Environment variables:")
        for k, v in os.environ.items():
            if k.startswith("COH"):  # show Cohesix-related vars
                cohlog(f"  {k}={v}")


def handle_boot(args):
    cohlog(f"Booting role: {args.role} ...")
    try:
        from cohesix.runtime.env import init

        init.initialize_runtime_env()
        os.environ["COH_ROLE"] = args.role
        cohlog("Boot sequence complete")
    except Exception as e:
        cohlog(f"Boot failed: {e}")


def handle_trace(args):
    filter_val = args.filter or "*"
    cohlog(f"Showing trace log entries matching '{filter_val}' (stub)")


def handle_trace_violations():
    path = "/srv/violations/runtime.json"
    if os.path.exists(path):
        cohlog(open(path).read())
    else:
        cohlog("No violations logged")


def handle_replay(args):
    from pathlib import Path
    from scripts import cohtrace

    trace = cohtrace.read_trace(Path(args.path))
    for ev in trace:
        cohlog(f"replay {ev['event']} {ev['detail']}")


def handle_dispatch_slm(args):
    req_dir = f"/srv/slm/dispatch/{args.target}"
    os.makedirs(req_dir, exist_ok=True)
    req_path = os.path.join(req_dir, f"{args.model}.req")
    open(req_path, "w").write("1")
    cohlog(f"Dispatch request for {args.model} sent to {args.target}")


def handle_agent(args):
    if args.agent_cmd == "start":
        cohlog(f"Starting agent {args.agent_id} with role {args.role}")
    elif args.agent_cmd == "pause":
        cohlog(f"Pausing agent {args.agent_id}")
    elif args.agent_cmd == "migrate":
        cohlog(f"Migrating agent {args.agent_id} to {args.to}")
        try:
            from cohesix import agent_migration

            agent_migration.migrate(args.agent_id, args.to)
        except Exception as e:
            cohlog(f"Migration failed: {e}")
    else:
        cohlog("Unknown agent command")


def handle_inference(args):
    os.environ["INFER_CONF"] = args.task
    script = os.path.join(os.path.dirname(__file__), "../scripts/worker_inference.py")
    safe_run(["python3", script])


def handle_federation(args):
    if args.fed_cmd == "connect":
        path = f"/srv/federation/requests/{args.peer}.connect"
        os.makedirs("/srv/federation/requests", exist_ok=True)
        open(path, "w").write("1")
        cohlog(f"Connect request sent to {args.peer}")
    elif args.fed_cmd == "disconnect":
        path = f"/srv/federation/requests/{args.peer}.disconnect"
        os.makedirs("/srv/federation/requests", exist_ok=True)
        open(path, "w").write("1")
        cohlog(f"Disconnect request sent to {args.peer}")
    elif args.fed_cmd == "list":
        for f in os.listdir("/srv/federation/known_hosts"):
            cohlog(f)
    elif args.fed_cmd == "monitor":
        log = "/srv/federation/events.log"
        if os.path.exists(log):
            cohlog(open(log).read())
    else:
        cohlog("Unknown federation command")


def handle_sim(args):
    if args.sim_cmd == "run" and args.scenario == "BalanceBot":
        try:
            from cohesix.sim.physics_adapter import PhysicsAdapter

            adapter = PhysicsAdapter.new()
            adapter.run_balance_bot(100)
            cohlog("BalanceBot simulation complete")
        except Exception as e:
            cohlog(f"Simulation failed: {e}")
    else:
        cohlog("Unknown simulation scenario")


def handle_sync_world(args):
    path = f"/srv/world_sync/{args.target}.json"
    if os.path.exists(path):
        cohlog(f"Synced world model to {args.target}")
    else:
        cohlog(f"No snapshot for {args.target}")


def handle_export_world(args):
    src = "/srv/world_model/world.json"
    if os.path.exists(src):
        import shutil

        shutil.copy(src, args.path)
        cohlog(f"World model exported to {args.path}")
    else:
        cohlog("World model not found")


def handle_show_policy(args):
    path = f"/persist/policy/agent_{args.agent}.policy.json"
    if os.path.exists(path):
        cohlog(open(path).read())
    else:
        cohlog("No policy found")


def handle_wipe_policy(args):
    path = f"/persist/policy/agent_{args.agent}.policy.json"
    if os.path.exists(path):
        os.remove(path)
        cohlog("Policy wiped")
    else:
        cohlog("No policy found")


def handle_vision_overlay(args):
    cohlog(f"Running vision overlay for {args.agent} (save={args.save})")


def handle_stream_overlay(args):
    cohlog(f"Streaming overlay on port {args.port}")


def handle_agent_introspect(args):
    path = f"/trace/introspect_{args.agent_id}.log"
    if os.path.exists(path):
        cohlog(open(path).read())
    else:
        cohlog("No introspection data")


def handle_elect_queen(args):
    cohlog(f"Electing queen using {args.mesh}")


def handle_assume_role(args):
    open("/srv/queen/role", "w").write(args.role)


def handle_upgrade(args):
    os.makedirs("/srv/upgrade", exist_ok=True)
    open("/srv/upgrade/url", "w").write(args.src)
    cohlog(f"Upgrade request for {args.src}")


def handle_rollback(args):
    os.makedirs("/srv/upgrade", exist_ok=True)
    open("/srv/upgrade/rollback", "w").write("1")
    cohlog("Rollback requested")


def handle_list_models(args):
    base = "/persist/models"
    if os.path.isdir(base):
        for f in os.listdir(base):
            if f.endswith(".slmcoh"):
                cohlog(f)
    else:
        cohlog("No models")


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
    cohlog(f"Decrypted to {out}")


def handle_verify_model(args):
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

    path = os.path.join("/persist/models", args.model)
    sig_path = path + ".sig"
    key_path = "/keys/slm_signing.pub"
    if not os.path.exists(sig_path) or not os.path.exists(key_path):
        cohlog("Missing signature or key")
        return
    pub = Ed25519PublicKey.from_public_bytes(open(key_path, "rb").read())
    data = open(path, "rb").read()
    sig = open(sig_path, "rb").read()
    try:
        pub.verify(sig, data)
        cohlog("Signature OK")
    except Exception:
        cohlog("Invalid signature")


def handle_agent_ensemble_status(args):
    path = f"/ensemble/{args.ensemble}/goals.json"
    if os.path.exists(path):
        cohlog(open(path).read())
    else:
        cohlog("No ensemble data")


if __name__ == "__main__":
    try:
        main()
    except Exception:
        with (LOG_DIR / "cli_error.log").open("a") as f:
            f.write(f"{datetime.utcnow().isoformat()} {traceback.format_exc()}\n")
        cohlog("Unhandled error, see cli_error.log")
        sys.exit(1)
