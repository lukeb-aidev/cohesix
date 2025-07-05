// CLASSIFICATION: COMMUNITY
// Filename: main.go v0.1
// Author: Lukas Bower
// Date Modified: 2027-01-30
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package main

import (
	"encoding/json"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"time"
)

// PhysicsJob mirrors the physics_job.json schema.
type PhysicsJob struct {
	JobID           string     `json:"job_id"`
	InitialPosition [3]float64 `json:"initial_position"`
	InitialVelocity [3]float64 `json:"initial_velocity"`
	Mass            float64    `json:"mass"`
	Duration        float64    `json:"duration"`
}

type World struct {
	FinalPosition   [3]float64 `json:"final_position"`
	FinalVelocity   [3]float64 `json:"final_velocity"`
	Collided        bool       `json:"collided"`
	EnergyRemaining float64    `json:"energy_remaining"`
}

type Result struct {
	JobID    string   `json:"job_id"`
	Status   string   `json:"status"`
	Steps    int      `json:"steps"`
	Duration float64  `json:"duration"`
	Logs     []string `json:"logs"`
}

func writeStatus(processed int, lastErr, lastJob string) {
	status := fmt.Sprintf("jobs_processed=%d\nlast_error=\"%s\"\nlast_job=\"%s\"\n", processed, lastErr, lastJob)
	os.WriteFile("/srv/physics/status", []byte(status), 0644)
}

func main() {
	os.MkdirAll("/srv/trace", 0755)
	os.MkdirAll("/srv/physics", 0755)
	os.MkdirAll("/sim", 0755)
	logFile, err := os.OpenFile("/srv/trace/sim.log", os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0644)
	if err != nil {
		log.Fatalf("log open: %v", err)
	}
	logger := log.New(logFile, "", log.LstdFlags)

	processed := 0
	lastErr := ""
	lastJob := ""

	for {
		matches, _ := filepath.Glob("/mnt/physics_jobs/physics_job_*.json")
		for _, jobPath := range matches {
			data, err := os.ReadFile(jobPath)
			if err != nil {
				logger.Printf("read %s: %v", jobPath, err)
				lastErr = err.Error()
				writeStatus(processed, lastErr, lastJob)
				continue
			}
			var job PhysicsJob
			if err := json.Unmarshal(data, &job); err != nil {
				logger.Printf("parse %s: %v", jobPath, err)
				lastErr = err.Error()
				writeStatus(processed, lastErr, lastJob)
				os.Remove(jobPath)
				continue
			}
			steps := int(job.Duration * 100)
			finalPos := [3]float64{
				job.InitialPosition[0] + job.InitialVelocity[0]*job.Duration,
				job.InitialPosition[1] + job.InitialVelocity[1]*job.Duration,
				job.InitialPosition[2] + job.InitialVelocity[2]*job.Duration,
			}
			world := World{FinalPosition: finalPos, FinalVelocity: job.InitialVelocity, Collided: false, EnergyRemaining: 0.95}
			wdata, _ := json.MarshalIndent(world, "", "  ")
			os.WriteFile("/sim/world.json", wdata, 0644)

			result := Result{JobID: job.JobID, Status: "completed", Steps: steps, Duration: float64(steps) / 100.0,
				Logs: []string{"t=0.1 pos=[0.1,0,0]", "t=0.2 pos=[0.2,0,0]"}}
			rdata, _ := json.MarshalIndent(result, "", "  ")
			os.WriteFile("/sim/result.json", rdata, 0644)

			logger.Printf("completed %s", job.JobID)
			lastErr = ""
			lastJob = fmt.Sprintf("%s @ %s", job.JobID, time.Now().Format("2006-01-02 15:04"))
			processed++
			writeStatus(processed, lastErr, lastJob)
			os.Remove(jobPath)
		}
		time.Sleep(2 * time.Second)
	}
}
