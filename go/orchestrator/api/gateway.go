// CLASSIFICATION: COMMUNITY
// Filename: gateway.go v0.1
// Author: Lukas Bower
// Date Modified: 2029-02-15
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package api

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"errors"
	"fmt"
	"net"
	"net/url"
	"os"
	"strings"
	"time"

	"cohesix/internal/orchestrator/rpc"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
)

const (
	// defaultOrchestratorEndpoint mirrors the queen orchestrator defaults.
	defaultOrchestratorEndpoint = "https://127.0.0.1:50051"

	envOrchAddr      = "COHESIX_ORCH_ADDR"
	envOrchCACert    = "COHESIX_ORCH_CA_CERT"
	envOrchClientCrt = "COHESIX_ORCH_CLIENT_CERT"
	envOrchClientKey = "COHESIX_ORCH_CLIENT_KEY"
)

// ClusterStateClient fetches the current orchestrator cluster state.
type ClusterStateClient interface {
	FetchClusterState(ctx context.Context) (*rpc.ClusterStateResponse, error)
}

// Gateway defines the behaviour expected from a gRPC-backed controller.
type Gateway interface {
	Controller
	ClusterStateClient
	Close() error
}

// GRPCGateway routes HTTP requests through the tonic gRPC orchestrator.
type GRPCGateway struct {
	client     rpc.OrchestratorServiceClient
	conn       *grpc.ClientConn
	rpcTimeout time.Duration
}

// NewGRPCGatewayFromEnv initialises the gateway using documented env vars.
func NewGRPCGatewayFromEnv(ctx context.Context, timeout time.Duration) (*GRPCGateway, error) {
	endpoint := strings.TrimSpace(os.Getenv(envOrchAddr))
	if endpoint == "" {
		endpoint = defaultOrchestratorEndpoint
	}
	return NewGRPCGateway(ctx, endpoint, timeout)
}

// NewGRPCGateway creates a gateway targeting a custom endpoint.
func NewGRPCGateway(ctx context.Context, endpoint string, timeout time.Duration) (*GRPCGateway, error) {
	if timeout <= 0 {
		timeout = 5 * time.Second
	}
	dialCtx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	conn, err := dialOrchestrator(dialCtx, endpoint)
	if err != nil {
		return nil, err
	}
	return &GRPCGateway{client: rpc.NewOrchestratorServiceClient(conn), conn: conn, rpcTimeout: timeout}, nil
}

// Close shuts down the underlying gRPC connection.
func (g *GRPCGateway) Close() error {
	if g == nil || g.conn == nil {
		return nil
	}
	return g.conn.Close()
}

// FetchClusterState queries the orchestrator for current cluster data.
func (g *GRPCGateway) FetchClusterState(ctx context.Context) (*rpc.ClusterStateResponse, error) {
	if g == nil {
		return nil, errors.New("grpc gateway not initialised")
	}
	ctx, cancel := context.WithTimeout(ctx, g.rpcTimeout)
	defer cancel()
	return g.client.GetClusterState(ctx, &rpc.ClusterStateRequest{})
}

// Execute forwards control commands to the orchestrator gRPC service.
func (g *GRPCGateway) Execute(ctx context.Context, req ControlRequest) error {
	if g == nil {
		return errors.New("grpc gateway not initialised")
	}
	ctx, cancel := context.WithTimeout(ctx, g.rpcTimeout)
	defer cancel()

	switch req.Command {
	case "assign-role":
		if req.WorkerID == "" || req.Role == "" {
			return errors.New("assign-role requires worker_id and role")
		}
		_, err := g.client.AssignRole(ctx, &rpc.AssignRoleRequest{WorkerId: req.WorkerID, Role: req.Role})
		return err
	case "update-trust":
		if req.WorkerID == "" || req.TrustLevel == "" {
			return errors.New("update-trust requires worker_id and trust_level")
		}
		_, err := g.client.UpdateTrust(ctx, &rpc.TrustUpdateRequest{WorkerId: req.WorkerID, Level: req.TrustLevel})
		return err
	case "schedule":
		if req.AgentID == "" {
			return errors.New("schedule requires agent_id")
		}
		schedule := &rpc.ScheduleRequest{AgentId: req.AgentID}
		if req.RequireGPU != nil {
			schedule.RequireGpu = *req.RequireGPU
		}
		_, err := g.client.RequestSchedule(ctx, schedule)
		return err
	default:
		return fmt.Errorf("unsupported command %q", req.Command)
	}
}

func dialOrchestrator(ctx context.Context, endpoint string) (*grpc.ClientConn, error) {
	if strings.TrimSpace(endpoint) == "" {
		endpoint = defaultOrchestratorEndpoint
	}
	parsed, err := url.Parse(endpoint)
	if err != nil {
		return nil, fmt.Errorf("invalid orchestrator endpoint %q: %w", endpoint, err)
	}
	if !strings.EqualFold(parsed.Scheme, "https") {
		return nil, fmt.Errorf("orchestrator endpoint %q must use https", endpoint)
	}
	hostPort := parsed.Host
	if hostPort == "" {
		return nil, fmt.Errorf("orchestrator endpoint %q missing host", endpoint)
	}
	if !strings.Contains(hostPort, ":") {
		hostPort = net.JoinHostPort(hostPort, "443")
	}

	creds, err := buildTLSCredentials(parsed.Hostname())
	if err != nil {
		return nil, err
	}

	return grpc.DialContext(ctx, hostPort, grpc.WithTransportCredentials(creds), grpc.WithBlock())
}

func buildTLSCredentials(serverName string) (credentials.TransportCredentials, error) {
	caPath := strings.TrimSpace(os.Getenv(envOrchCACert))
	clientCertPath := strings.TrimSpace(os.Getenv(envOrchClientCrt))
	clientKeyPath := strings.TrimSpace(os.Getenv(envOrchClientKey))
	if caPath == "" || clientCertPath == "" || clientKeyPath == "" {
		return nil, errors.New("missing orchestrator TLS credentials")
	}

	caData, err := os.ReadFile(caPath)
	if err != nil {
		return nil, fmt.Errorf("read ca cert: %w", err)
	}
	pool := x509.NewCertPool()
	if !pool.AppendCertsFromPEM(caData) {
		return nil, errors.New("failed to parse orchestrator CA certificate")
	}

	certificate, err := tls.LoadX509KeyPair(clientCertPath, clientKeyPath)
	if err != nil {
		return nil, fmt.Errorf("load client cert: %w", err)
	}

	tlsCfg := &tls.Config{
		MinVersion:   tls.VersionTLS13,
		Certificates: []tls.Certificate{certificate},
		RootCAs:      pool,
		ServerName:   serverName,
	}
	return credentials.NewTLS(tlsCfg), nil
}
