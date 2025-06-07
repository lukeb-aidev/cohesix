
#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: cohcli.py v0.5
# Date Modified: 2025-07-07
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

    return parser.parse_args()

def main():
    args = parse_args()

    if args.command == "status":
        handle_status(args)
    elif args.command == "boot":
        handle_boot(args)
    elif args.command == "trace":
        handle_trace(args)
    elif args.command == "agent":
        handle_agent(args)
    elif args.command == "sim":
        handle_sim(args)
    elif args.command == "federation":
        handle_federation(args)
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

if __name__ == "__main__":
    main()
