// Author: Lukas Bower
// Date Modified: 2029-01-15

use cohesix::orchestrator::protocol::{
    AssignRoleRequest, ClusterStateRequest, GpuTelemetry, HeartbeatRequest, JoinRequest,
    ScheduleRequest, TrustUpdateRequest,
};
use cohesix::queen::orchestrator::{QueenOrchestrator, SchedulePolicy};
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::oneshot;

#[test]
fn orchestrator_grpc_flows() {
    let rt = Runtime::new().expect("runtime");
    rt.block_on(async {
        let orchestrator = QueenOrchestrator::new(1, SchedulePolicy::GpuPriority);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let server = tokio::spawn(orchestrator.clone().serve_with_listener(listener, async {
            let _ = shutdown_rx.await;
        }));

        let endpoint = format!("http://{}", addr);
        let mut client = QueenOrchestrator::connect_client(&endpoint)
            .await
            .expect("client");
        client
            .join(JoinRequest {
                worker_id: "worker-alpha".into(),
                ip: "10.1.1.10".into(),
                role: "DroneWorker".into(),
                capabilities: vec!["cuda".into()],
                trust: "green".into(),
            })
            .await
            .expect("join");
        client
            .heartbeat(HeartbeatRequest {
                worker_id: "worker-alpha".into(),
                timestamp: 0,
                status: "running".into(),
                gpu: Some(GpuTelemetry {
                    perf_watt: 12.5,
                    mem_total: 24,
                    mem_free: 12,
                    last_temp: 60,
                    gpu_capacity: 12,
                    current_load: 3,
                    latency_score: 5,
                }),
            })
            .await
            .expect("heartbeat");

        let schedule = client
            .request_schedule(ScheduleRequest {
                agent_id: "job-1".into(),
                require_gpu: true,
            })
            .await
            .expect("schedule")
            .into_inner();
        assert!(schedule.assigned);
        assert_eq!(schedule.worker_id, "worker-alpha");

        client
            .assign_role(AssignRoleRequest {
                worker_id: "worker-alpha".into(),
                role: "ComputeNode".into(),
            })
            .await
            .expect("assign");

        client
            .update_trust(TrustUpdateRequest {
                worker_id: "worker-alpha".into(),
                level: "yellow".into(),
            })
            .await
            .expect("trust");

        let state = client
            .get_cluster_state(ClusterStateRequest {})
            .await
            .expect("state")
            .into_inner();
        assert_eq!(state.workers.len(), 1);
        assert_eq!(state.workers[0].trust, "yellow");
        assert_eq!(state.workers[0].role, "ComputeNode");

        tokio::time::sleep(Duration::from_secs(2)).await;
        let state_after = client
            .get_cluster_state(ClusterStateRequest {})
            .await
            .expect("state2")
            .into_inner();
        assert_eq!(state_after.workers[0].status, "restarting");

        let _ = shutdown_tx.send(());
        server.await.expect("server").expect("shutdown");
    });
}
