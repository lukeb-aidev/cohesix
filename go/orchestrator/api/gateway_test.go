// CLASSIFICATION: COMMUNITY
// Filename: gateway_test.go v0.1
// Author: Lukas Bower
// Date Modified: 2029-02-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package api

import (
	"context"
	"testing"
	"time"

	"cohesix/internal/orchestrator/rpc"
	"google.golang.org/grpc"
)

type fakeOrchestratorClient struct {
	lastAssignRole   *rpc.AssignRoleRequest
	lastTrustUpdate  *rpc.TrustUpdateRequest
	lastSchedule     *rpc.ScheduleRequest
	clusterStateResp *rpc.ClusterStateResponse
}

func (f *fakeOrchestratorClient) Join(context.Context, *rpc.JoinRequest, ...grpc.CallOption) (*rpc.JoinResponse, error) {
	return nil, nil
}

// Implement required interface methods with minimal behaviour.
func (f *fakeOrchestratorClient) Heartbeat(context.Context, *rpc.HeartbeatRequest, ...grpc.CallOption) (*rpc.HeartbeatResponse, error) {
	return nil, nil
}

func (f *fakeOrchestratorClient) RequestSchedule(_ context.Context, in *rpc.ScheduleRequest, _ ...grpc.CallOption) (*rpc.ScheduleResponse, error) {
	f.lastSchedule = in
	return &rpc.ScheduleResponse{}, nil
}

func (f *fakeOrchestratorClient) AssignRole(_ context.Context, in *rpc.AssignRoleRequest, _ ...grpc.CallOption) (*rpc.AssignRoleResponse, error) {
	f.lastAssignRole = in
	return &rpc.AssignRoleResponse{Updated: true}, nil
}

func (f *fakeOrchestratorClient) UpdateTrust(_ context.Context, in *rpc.TrustUpdateRequest, _ ...grpc.CallOption) (*rpc.TrustUpdateResponse, error) {
	f.lastTrustUpdate = in
	return &rpc.TrustUpdateResponse{}, nil
}

func (f *fakeOrchestratorClient) GetClusterState(context.Context, *rpc.ClusterStateRequest, ...grpc.CallOption) (*rpc.ClusterStateResponse, error) {
	if f.clusterStateResp == nil {
		f.clusterStateResp = &rpc.ClusterStateResponse{}
	}
	return f.clusterStateResp, nil
}

func (f *fakeOrchestratorClient) Close() error { return nil }

func TestGRPCGatewayExecutesAssignRole(t *testing.T) {
	client := &fakeOrchestratorClient{}
	gateway := &GRPCGateway{client: client, rpcTimeout: time.Second}
	req := ControlRequest{Command: "assign-role", WorkerID: "worker-a", Role: "QueenPrimary"}
	if err := gateway.Execute(context.Background(), req); err != nil {
		t.Fatalf("execute: %v", err)
	}
	if client.lastAssignRole == nil || client.lastAssignRole.WorkerId != "worker-a" {
		t.Fatalf("assign role not invoked: %+v", client.lastAssignRole)
	}
	if client.lastAssignRole.Role != "QueenPrimary" {
		t.Fatalf("role mismatch: %v", client.lastAssignRole.Role)
	}
}

func TestGRPCGatewayExecutesUpdateTrust(t *testing.T) {
	client := &fakeOrchestratorClient{}
	gateway := &GRPCGateway{client: client, rpcTimeout: time.Second}
	req := ControlRequest{Command: "update-trust", WorkerID: "worker-a", TrustLevel: "amber"}
	if err := gateway.Execute(context.Background(), req); err != nil {
		t.Fatalf("execute: %v", err)
	}
	if client.lastTrustUpdate == nil || client.lastTrustUpdate.WorkerId != "worker-a" {
		t.Fatalf("update trust not invoked: %+v", client.lastTrustUpdate)
	}
	if client.lastTrustUpdate.Level != "amber" {
		t.Fatalf("trust level mismatch: %v", client.lastTrustUpdate.Level)
	}
}

func TestGRPCGatewayExecutesSchedule(t *testing.T) {
	client := &fakeOrchestratorClient{}
	gateway := &GRPCGateway{client: client, rpcTimeout: time.Second}
	requireGPU := true
	req := ControlRequest{Command: "schedule", AgentID: "cohrun-test", RequireGPU: &requireGPU}
	if err := gateway.Execute(context.Background(), req); err != nil {
		t.Fatalf("execute: %v", err)
	}
	if client.lastSchedule == nil || client.lastSchedule.AgentId != "cohrun-test" {
		t.Fatalf("schedule not invoked: %+v", client.lastSchedule)
	}
	if !client.lastSchedule.RequireGpu {
		t.Fatalf("expected GPU requirement to propagate")
	}
}

func TestGRPCGatewayRejectsUnknownCommand(t *testing.T) {
	client := &fakeOrchestratorClient{}
	gateway := &GRPCGateway{client: client, rpcTimeout: time.Second}
	err := gateway.Execute(context.Background(), ControlRequest{Command: "noop"})
	if err == nil {
		t.Fatalf("expected error for unsupported command")
	}
}
