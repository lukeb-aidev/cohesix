# Evidence timeline

events: 2

- ticket=fixture-1:attempt-1:hive-a:hive-b action=systemd.restart state=queued source_hive=hive-a target_hive=hive-b stream=host/tickets/spec target= relay_hop=1
- ticket=fixture-1:attempt-1:hive-a:hive-b action=systemd.restart state=succeeded source_hive=hive-a target_hive=hive-b stream=host/tickets/status target= relay_hop=1
