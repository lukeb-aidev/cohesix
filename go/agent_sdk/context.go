// CLASSIFICATION: COMMUNITY
// Filename: context.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

package agentsdk

import (
	"encoding/json"
	"os"
)

type AgentContext struct {
	Role          string
	Uptime        string
	LastGoal      map[string]any
	WorldSnapshot map[string]any
}

func readFile(path string) []byte {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil
	}
	return data
}

func New() *AgentContext {
	ctx := &AgentContext{
		Role:          "Unknown",
		Uptime:        "0",
		LastGoal:      map[string]any{},
		WorldSnapshot: map[string]any{},
	}
	if b := readFile("/srv/agent_meta/role.txt"); b != nil {
		ctx.Role = string(b)
	}
	if b := readFile("/srv/agent_meta/uptime.txt"); b != nil {
		ctx.Uptime = string(b)
	}
	if b := readFile("/srv/agent_meta/last_goal.json"); b != nil {
		json.Unmarshal(b, &ctx.LastGoal)
	}
	if b := readFile("/srv/world_state/world.json"); b != nil {
		json.Unmarshal(b, &ctx.WorldSnapshot)
	}
	return ctx
}
